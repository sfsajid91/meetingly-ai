//! Client-owned PipeWire links from explicitly selected output ports to capture inputs.
//!
//! Add `mod links;` in `main.rs`, then call [`SourceLinks::new`] during mainloop
//! setup, passing the existing [`MainLoopRc`](pw::main_loop::MainLoopRc) and
//! exact source/target selectors.
//! Keep the returned value alive for as long as the links should exist. Its
//! registry listener, Core connection, and link proxies are retained together;
//! dropping it destroys this client's non-lingering links. Resolution starts
//! after the initial registry snapshot and retries missing ports as globals
//! appear; names never fall back to another device.
//!
//! ```ignore
//! let links = links::SourceLinks::new(&mainloop, [
//!     links::PortLink::new(
//!         links::PortSelector::new("selected-mic-node", "capture_FL"),
//!         links::PortSelector::new("byok-filter-proof", "mic"),
//!     ),
//!     links::PortLink::new(
//!         links::PortSelector::new("selected-monitor-node", "monitor_FL"),
//!         links::PortSelector::new("byok-filter-proof", "monitor"),
//!     ),
//! ])?;
//! // After mainloop.run() starts, check links.status() for Pending, Linked,
//! // or a specific MissingSource/MissingTarget/AmbiguousPort result.
//! ```
//!
//! Selectors match both `node.name` and `port.name`, with the expected port
//! direction. PipeWire node and port global IDs are passed to `link-factory`;
//! no external process or persistent link is involved.

use pipewire as pw;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// A node/port pair identifying exactly one port by its PipeWire properties.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortSelector {
    pub node_name: String,
    pub port_name: String,
}

impl PortSelector {
    pub fn new(node_name: impl Into<String>, port_name: impl Into<String>) -> Self {
        Self {
            node_name: node_name.into(),
            port_name: port_name.into(),
        }
    }
}

/// One explicitly selected output source and its input destination.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortLink {
    pub source: PortSelector,
    pub target: PortSelector,
}

impl PortLink {
    pub fn new(source: PortSelector, target: PortSelector) -> Self {
        Self { source, target }
    }
}

/// Current source-resolution and link-creation state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkStatus {
    /// The initial registry burst has not completed yet.
    Pending,
    /// Link creation was requested for both selected port pairs.
    Linked,
    /// No matching output source port was present.
    MissingSource(PortSelector),
    /// No matching input destination port was present.
    MissingTarget(PortSelector),
    /// More than one matching port exists; no links were created.
    AmbiguousPort(PortSelector),
    /// PipeWire rejected link creation; no partial links are retained.
    CreationFailed(String),
}

#[derive(Clone, Debug)]
struct GlobalPort {
    node_id: u32,
    port_name: String,
    direction: String,
}

#[derive(Default)]
struct Globals {
    nodes: HashMap<u32, String>,
    ports: HashMap<u32, GlobalPort>,
}

struct Shared {
    selections: [PortLink; 2],
    globals: Globals,
    snapshot_complete: bool,
    status: LinkStatus,
    links: Vec<pw::link::Link>,
}

/// Owns two client-created links and the PipeWire objects/listeners backing them.
///
/// Construct this on the same PipeWire mainloop as the capture filter. The
/// connection is independent but shares the loop, so callbacks run normally
/// from the caller's `mainloop.run()`.
pub struct SourceLinks {
    // Listeners unregister before their registry/core dependencies are dropped.
    _registry_listener: pw::registry::Listener,
    _core_listener: pw::core::Listener,
    _registry: pw::registry::RegistryRc,
    _core: pw::core::CoreRc,
    _context: pw::context::ContextRc,
    shared: Rc<RefCell<Shared>>,
}

impl SourceLinks {
    /// Start resolving and linking two selected source ports to two input ports.
    ///
    /// Link creation starts after the initial registry sync. Missing ports are
    /// re-evaluated as registry globals arrive.
    pub fn new(
        mainloop: &pw::main_loop::MainLoopRc,
        selections: [PortLink; 2],
    ) -> Result<Self, pw::Error> {
        let context = pw::context::ContextRc::new(mainloop, None)?;
        let core = context.connect_rc(None)?;
        let registry = core.get_registry_rc()?;
        let shared = Rc::new(RefCell::new(Shared {
            selections,
            globals: Globals::default(),
            snapshot_complete: false,
            status: LinkStatus::Pending,
            links: Vec::with_capacity(2),
        }));

        let global_state = shared.clone();
        let global_core = core.clone();
        let registry_listener = registry
            .add_listener_local()
            .global(move |global| {
                let Some(props) = global.props else {
                    return;
                };
                let mut state = global_state.borrow_mut();
                match global.type_ {
                    pw::types::ObjectType::Node => {
                        if let Some(name) = props.get("node.name") {
                            // Node global IDs are the IDs required by link-factory.
                            state.globals.nodes.insert(global.id, name.to_owned());
                        }
                    }
                    pw::types::ObjectType::Port => {
                        let (Some(node_id), Some(port_name), Some(direction)) = (
                            props.get("node.id"),
                            props.get("port.name"),
                            props.get("port.direction"),
                        ) else {
                            return;
                        };
                        if let Ok(node_id) = node_id.parse::<u32>() {
                            state.globals.ports.insert(
                                global.id,
                                GlobalPort {
                                    node_id,
                                    port_name: port_name.to_owned(),
                                    direction: direction.to_owned(),
                                },
                            );
                        }
                    }
                    _ => {}
                }
                let ready = state.snapshot_complete;
                drop(state);
                if ready {
                    resolve_and_link(&global_core, &global_state);
                }
            })
            .global_remove({
                let removal_state = shared.clone();
                move |id| {
                    let mut state = removal_state.borrow_mut();
                    state.globals.nodes.remove(&id);
                    state.globals.ports.remove(&id);
                }
            })
            .register();

        // Register the listener after requesting sync, matching PipeWire's
        // roundtrip pattern while ensuring this callback observes that sync.
        let pending = core.sync(0)?;
        let completion_state = shared.clone();
        let completion_core = core.clone();
        let core_listener = core
            .add_listener_local()
            .done(move |id, seq| {
                if id != pw::core::PW_ID_CORE || seq != pending {
                    return;
                }
                completion_state.borrow_mut().snapshot_complete = true;
                resolve_and_link(&completion_core, &completion_state);
            })
            .register();

        Ok(Self {
            _context: context,
            _core: core,
            _registry: registry,
            _registry_listener: registry_listener,
            _core_listener: core_listener,
            shared,
        })
    }

    /// Current resolution state. It becomes final after the initial registry sync.
    pub fn status(&self) -> LinkStatus {
        self.shared.borrow().status.clone()
    }
}

impl Drop for SourceLinks {
    fn drop(&mut self) {
        self.shared.borrow_mut().links.clear();
    }
}

fn resolve_and_link(core: &pw::core::CoreRc, shared: &Rc<RefCell<Shared>>) {
    let mut state = shared.borrow_mut();
    if !matches!(state.status, LinkStatus::Pending | LinkStatus::MissingSource(_) | LinkStatus::MissingTarget(_)) {
        return;
    }

    let mut resolved = [((0u32, 0u32), (0u32, 0u32)); 2];
    let selections = state.selections.clone();
    for (index, selection) in selections.iter().enumerate() {
        let source = match find_port(
            &state.globals,
            &selection.source,
            "out",
        ) {
            PortMatch::Missing => {
                state.status = LinkStatus::MissingSource(selection.source.clone());
                return;
            }
            PortMatch::Ambiguous => {
                state.status = LinkStatus::AmbiguousPort(selection.source.clone());
                return;
            }
            PortMatch::Found(ids) => ids,
        };
        let target = match find_port(
            &state.globals,
            &selection.target,
            "in",
        ) {
            PortMatch::Missing => {
                state.status = LinkStatus::MissingTarget(selection.target.clone());
                return;
            }
            PortMatch::Ambiguous => {
                state.status = LinkStatus::AmbiguousPort(selection.target.clone());
                return;
            }
            PortMatch::Found(ids) => ids,
        };
        resolved[index] = (source, target);
    }

    for (source, target) in resolved {
        let properties = pw::properties::properties! {
            "link.output.node" => source.0.to_string(),
            "link.output.port" => source.1.to_string(),
            "link.input.node" => target.0.to_string(),
            "link.input.port" => target.1.to_string(),
            // Unlike object.linger=1, this proxy's lifetime owns the link.
            "object.linger" => "0",
        };
        match core.create_object::<pw::link::Link>("link-factory", &properties) {
            Ok(link) => state.links.push(link),
            Err(error) => {
                state.links.clear();
                state.status = LinkStatus::CreationFailed(format!("{error:?}"));
                return;
            }
        }
    }
    state.status = LinkStatus::Linked;
}

enum PortMatch {
    Missing,
    Ambiguous,
    Found((u32, u32)),
}

fn find_port(globals: &Globals, selector: &PortSelector, direction: &str) -> PortMatch {
    let mut found = None;
    for (port_id, port) in &globals.ports {
        if port.port_name != selector.port_name || port.direction != direction {
            continue;
        }
        let Some(node_name) = globals.nodes.get(&port.node_id) else {
            continue;
        };
        if node_name != &selector.node_name {
            continue;
        }
        if found.is_some() {
            return PortMatch::Ambiguous;
        }
        found = Some((port.node_id, *port_id));
    }
    found.map_or(PortMatch::Missing, PortMatch::Found)
}
