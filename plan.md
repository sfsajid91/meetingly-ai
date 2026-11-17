# Rust and Tauri BYOK rebuild plan

Build the local-first BYOK meeting assistant as a Tauri desktop app with native Rust services. Keep the existing web interface where useful and use Bun for frontend dependencies and commands. Remove Electron, Meetingly cloud features, local transcription, obsolete scripts, and unused code after their replacements pass verification. Ship the complete cutover as one reviewable change, BYOK-1.

## How to read this

One box is one unit of work and names the evidence that checks it. Check a box only when its evidence exists. Work in the phase order in BYOK-1 and stop on a failed check. The execution workflow is `playbooks/autopilot-stack.md` if a pull request is opened. The operator lands it. No merge is authorized by this plan.

Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

## Implementation status

The operator requested a fresh provider setup and waived migration of the legacy `ownKey`. Preserve the Electron data directory and continue to migrate settings, prompts, shortcuts, and meeting history. Do not decrypt or import legacy credentials.

The rebuild is incomplete. The native audio gate remains open, and provider integration must not start until it passes.

- The implementation baseline is `fba3a2e43eb077a01409373553e44189d68d10b2` on `byok-only`. The baseline frontend suite passed 123 tests in 16 files. The host uses Wayland, and the Electron data directory is `/home/sajid/.config/Meetingly`. The environment and audio sources are recorded in `/tmp/byok-1/baseline.txt`.
- Rust and Cargo 1.100.0 are installed and pinned in `rust-toolchain.toml`. Bun 1.4.4 generated `app/bun.lock`, and `bun install --frozen-lockfile` passed. `app/src-tauri/Cargo.lock` is generated. Neither lockfile has been committed.
- The Tauri 2 bootstrap exists in `app/src-tauri/` with a separate application identifier. `cargo check --locked` passed for the bootstrap before the later CSP change. The Tauri-mode Vite asset build passed with no `node` executable on `PATH`. The renderer bridge, Tauri development session, production package, and full cutover remain unverified.
- The reviewed community Tauri IPC, security, and build skills were installed from `full-stack-skills/tauri-skills` revision `77b57c4913873a0f8cab7db45116038dc18a27f0`. Existing unrelated agent configuration was preserved.
- A temporary Linux Secret Service entry was stored, retrieved, and removed using the Rust keyring crate. This proves only access to the current keychain, not the application's credential workflow. The probe is in `/tmp/byok-1/keyring-proof/`.
- The selected microphone and monitor were captured concurrently on their assigned channels. A Rust probe also recorded a single 16 kHz stereo stream from a temporary server-side mixer while a monitor test tone played. The physical microphone signal clipped heavily. The mixer is not a release candidate because its server-loaded modules can outlive their creator and leave capture active after a crash.
- The independent-stream PipeWire prototype failed during monitor playback when the graph buffer size changed and its estimated timestamps reordered delivered buffers. The trace is `/tmp/byok-1/audio-proof/transition.stderr`.
- A client-owned PipeWire filter prototype captured microphone and monitor inputs in one graph callback. A monitor test tone appeared on the monitor channel, and the microphone signal clipped heavily. After either source link was removed, the revised prototype identified the missing source while capture was running. The filter ports and links disappeared after normal exit and after `SIGKILL`. A shared monitor signal produced identical samples on both filter inputs. Evidence is in `/tmp/byok-1/audio-proof/filter-test.stderr`, `/tmp/byok-1/audio-proof/filter-disconnect-live.stderr`, and `/tmp/byok-1/audio-proof/filter-monitor-disconnect.stderr`. The audio gate remains open. Physical alignment, underrun reporting, production source linking, and a bounded real-time-safe implementation remain unproven.
- A second physical-device run reduced the PulseAudio source volume to 10% and restored its original value afterward. The microphone channel still had 5,130 near-full-scale samples out of 140,288. This does not establish whether clipping originates in the device, the graph, or the probe. The capture is `/tmp/byok-1/audio-proof/repo-lowgain.raw`.
- Simultaneous float32 captures from the filter input and PulseAudio microphone source both reached an absolute peak of 2.505 before PCM16 conversion. The filter measured 179,012 input samples at or above an absolute value of 1.0; the separate PulseAudio capture measured 145,862. The probe clamps values outside the PCM16 range, so conversion clips some of this input. The origin of the high input level and any earlier clipping remain unknown. Evidence is in `/tmp/byok-1/audio-proof/input-diagnostic.stderr` and the sensitive `/tmp/byok-1/audio-proof/pulse-mic-check.raw`.
- The selected source uses the headset microphone and ALSA card 1 `Capture` control at 30 dB. Temporarily reducing that control lowered the filter input peak from 2.505 to approximately 1.0, but 9,804 of 138,240 resulting PCM frames remained near full scale. The test does not establish whether the signal was already clipped upstream. The original ALSA and PulseAudio volume values were restored. Evidence is `/tmp/byok-1/audio-proof/gain-diagnostic.stderr` and the sensitive `/tmp/byok-1/audio-proof/gain-diagnostic.raw`.
- A startup check linked only the monitor and left the microphone unlinked. The probe did not report the missing microphone while it was running. The audio gate failed this check and remains open.
- The startup failure is fixed in the probe. Missing sources are reported at startup and after disconnect. A shared chirp sent to both ports yielded zero relative alignment. A preallocated 256-block ring hands callback records to the writer. Initial evidence is in `/tmp/byok-1/audio-proof/gate/`.
- Removed an unsupported heuristic that suppressed large graph-clock jumps. A 0.5-second `SIGSTOP` of the probe produced one 22,528-frame graph-position discontinuity, with `xrun_cycles=0`; this detects the interruption but does not prove an XRUN notification or establish the number of lost audio frames. Evidence is `/tmp/byok-1/audio-proof/interrupt-check.stderr`.
- The probe now uses a 63-tap low-pass FIR rather than the earlier box decimator. In an isolated null-sink capture, a 1 kHz tone measured 5,656.82 PCM RMS and a 12 kHz tone fell below the PCM16 measurement floor; both output channels were identical. A separate diagnostic copy delayed one callback by 100 ms and recorded a graph-position discontinuity but no `XRUN_RECOVER` flag. Evidence is in `/tmp/byok-fir.907xuS4N/` and `/tmp/byok-xrun.yQCwcq46/`.
- Explicit PipeWire source ports are now resolved and linked by the probe. An isolated null-sink test passed for two selected ports, missing-source rejection, and normal cleanup; real microphone and monitor ports were linked and removed after `SIGKILL`. The two filtered channels matched on a shared chirp, with correlation 0.9352. A 0.5-second process pause produced a graph-clock discontinuity reported while capture was still running. Evidence is in `/tmp/byok-links.LHH174oh/`, `/tmp/byok-physical.nr4T75Fj/`, `/tmp/byok-1/audio-proof/fir-sync/`, and `/tmp/byok-1/audio-proof/live-discontinuity.stderr`.
- With the operator positioning an earcup near the microphone, the monitor chirp was detected but neither the headset nor built-in microphone produced a reliable reference match. Both microphone recordings contained near-full-scale samples, including after temporarily reducing hardware gain. Original microphone routing and volume were restored. Sensitive recordings are in `~/.local/share/meetingly/audio-proof/`. Physical latency and the origin of the microphone signal remain unproven.
- A subsequent headset speech capture measured an input peak of 0.312 and no near-full-scale PCM samples. A repeated headset acoustic chirp test remained inconclusive: monitor correlation was 0.9435, microphone correlation was 0.0172, and the microphone again reached the PCM limit. The speech recording and chirp evidence are in `~/.local/share/meetingly/audio-proof/headset-speech.OtRwLq0o/` and `~/.local/share/meetingly/audio-proof/headset-final.6Xbw282J/`.
- The authorized physical chirp was detected on the monitor (correlation 0.9435) but not on the selected headset microphone (correlation −0.0035). The microphone recording was not clipped in that run, but it did not yield a match. A deliberate 250 ms delay in a diagnostic callback produced clock gaps and overlaps but no `capture_xrun` event or `XRUN_RECOVER` cycle. Evidence is under `~/.local/share/meetingly/audio-proof/authorized-physical.NgZBqfst/` and `~/.local/share/meetingly/audio-proof/authorized-xrun.SFjcZUSo/`. These checks fail to establish physical alignment and XRUN reporting.
- At the operator's request, a five-second excerpt from `satiate.mp3` was played through the current headphones while capturing the connected headset mic and the system monitor. The monitor captured the excerpt. The mic-to-monitor cross-correlation peaked at 0.0935, too weak for a reliable delay measurement; neither channel clipped. The run also recorded one graph-clock gap and one repeated position, but no XRUN flag. Sensitive evidence is in `~/.local/share/meetingly/audio-proof/music-alignment.K061OPBc/`.
- Repeating the operator-requested music capture with the sink temporarily raised from 46% to 75% produced a mic/monitor cross-correlation peak of 0.3606 at 24.44 ms. Both channels remained below clipping. This is suggestive but below the 0.5 match threshold; physical alignment is still inconclusive. The original sink volume was restored. Sensitive capture is `~/.local/share/meetingly/audio-proof/music-alignment-louder.xQivcZNh/`.
- The frontend suite passes 123 tests in 16 files at the current head, and `cd app && bun run --bun typecheck` passes. The Tauri-mode Vite build still passes without emitting Electron bundles. The Tauri renderer bridge, development session, production package, and full cutover remain unverified.
- The operator reported noise in the initial 10-second WAV. File analysis found 7,010 of 160,000 microphone samples at the PCM rail (4.38%). The active headset path has ALSA capture gain +30 dB and headset boost +10 dB. With those temporarily lowered to +15 dB and 0 dB, a new 10-second capture had no rail samples and a peak input of 0.257. The WAV was boosted by 18 dB for listening, still with zero rail samples, and the operator reported this version sounded great. Original mixer settings were restored. This supports lower analog gain with controlled digital gain; it does not justify changing global mixer settings silently. Evidence is under `~/.local/share/meetingly/audio-proof/reduced-gain-20261117-171351/`.
- The PipeWire capture proof, verifier, and related gate evidence were committed as `176016f57f5eb330f72569fe793cf7dab6fd2c47` (`feat(audio): add PipeWire capture proof`). The commit does not mean the physical-alignment or XRUN gate passed.

The reusable filter probe is in `app/tools/audio-capture-proof/`. It requires PipeWire development headers, which are now available on the target host. It captures 48 kHz graph audio into interleaved 16 kHz stereo PCM16. Run it with explicit selected source ports as documented in its README, or link manually. The probe is not integrated into the application. Its output contains microphone audio and must be treated as sensitive.

The `/tmp/byok-1/audio-proof/` artifacts include microphone recordings. Treat them as sensitive local data.

## Program checklist

### Arm the program

- [x] Start implementation only after the operator requests it. Preserve the existing `byok-only` branch and the untracked guide.
- [x] Record the baseline commit, branch, test results, Linux audio devices, display server, and existing app data location. Evidence: `/tmp/byok-1/baseline.txt`.
- [ ] If an ongoing autonomous program is requested, use `/loop 1h` for an audit and report a status message only when a tracked state changes. No autonomous program has been requested for this plan.

### Spawn owners

- [x] Assign one integration owner for BYOK-1. Parallelize only independent research or implementation with explicit file ownership. The coordinating assistant owns integration.
- [ ] Keep the phase order below. Do not merge or release partial work as the finished BYOK build.

### PR mechanics

- [ ] Run the focused checks after each phase. Run the full test suite, typecheck, and packaging checks once at the end.
- [ ] Do not commit user-owned files or existing local changes. Open a PR only if requested.

### Verdict and merge

- [ ] Review the packaged Tauri app, network evidence, migration results, and test results at the final head.
- [ ] Hand the finished change to the operator. Do not merge without a separate request.

### Boot recipe

- [ ] Use isolated copies of the Electron and Tauri data directories and test accounts with limited credentials. Never place API keys in logs, arguments, screenshots, or evidence.
- [ ] Run `cd app && bun install --frozen-lockfile`, `bun run --bun tauri dev`, and the packaged build after the toolchain phase. Record the commands, tool versions, and environment.
- [ ] Drive the Tauri WebKit UI with WebDriver or accessible desktop automation. Use the target Linux desktop and real audio devices for the final capture proof. Save evidence under `/tmp/byok-1/`.

## Rebuild the BYOK desktop application (BYOK-1)

**Depends on.** None. Set up Rust, Bun, Tauri, and relevant agent skills first. Prove native capture and secure storage for new credentials before completing the cutover.

**Files.**

- [ ] Create `app/src-tauri/` for the Rust application, commands, services, configuration, capabilities, tests, and native build resources.
- [ ] Migrate `app/electron/main.ts`, `app/electron/ipc/`, `app/electron/preload.ts`, `app/electron/services/`, and their callers to Rust or Tauri APIs. Preserve required behavior and remove the Electron code after verification.
- [ ] Adapt `app/src/`, `app/shared/`, and the Vite configuration to Tauri and its webview. Update `app/package.json`, the Bun lockfile, build scripts, tests, packaging, and project documentation.

**Build.**
- [ ] Inventory installed Rust, `rustup`, Cargo, Bun, and the Fedora Tauri prerequisites. Install missing components with the official setup guides. Record versions and commands. Pin a stable Rust toolchain in `rust-toolchain.toml` and commit the generated Cargo and Bun lockfiles.
- [x] Review the community Tauri skills linked in Appendix D before installation. Install only the skills needed for Tauri IPC, security, Rust, and packaging in the configured agent skill location. The installed IPC, security, and build skills use revision `77b57c4913873a0f8cab7db45116038dc18a27f0`. Unrelated agent configuration remains unchanged.
- [ ] Initialize Tauri 2 around the existing Vite frontend. Configure Bun as the package manager and use `bun run --bun` where the script must execute under Bun. Verify development, tests, typecheck, and production build without a Node runtime. Fix incompatible dependencies rather than silently switching runtimes.
- [ ] Define the target boundary. Rust owns capture, provider networking, session state, persistence, credentials, shortcuts, and native operating system access. Tauri commands and channels expose narrow operations and events to the existing web UI. Keep API keys and raw provider credentials out of the webview.
- [ ] Prove simultaneous native microphone and monitor capture on the target Linux desktop before provider integration. Evaluate `cpal`, `pipewire`, or `libpulse-binding` against actual source selection. Select a maintained Rust crate that passes the proof. Capture sources independently, align or resample them to 16 kHz, and preserve left microphone and right monitor channels without blocking audio callbacks. Report disconnects, underruns, and missing sources.
- [ ] Implement provider networking in Rust using maintained crates. Evaluate `tokio`, `tokio-tungstenite`, `reqwest`, and a compatible Rust OpenAI client. Use direct Rust HTTP and server-sent event handling when an existing client cannot support the required endpoint and streaming behavior. Do not retain a Node or Bun sidecar in the shipped app.
- [x] Skip migration of legacy Electron `safeStorage` credentials and `ownKey` as requested by the operator. Leave the original encrypted data untouched.
- [ ] Migrate copied settings and meetings into the Tauri data directory with versioned, atomic writes and a recoverable backup. Make retries idempotent. Verify the old data remains readable after interruption, and document rollback to the untouched Electron data directory.

- [ ] Define the persistent shape first. Store provider records with stable IDs, type, label, endpoint, model, and an OS-keychain reference. Store one selected transcription provider and one selected brain provider. Allow multiple configured providers of each supported type. Keep secrets in the Rust process and expose only `hasKey` to the webview. Use a maintained native keyring crate with an actual secure OS backend. Reject insecure fallback.
- [ ] Provide direct Deepgram streaming in Rust as the initial transcription integration. Send 16 kHz interleaved PCM16 with `channels=2` and `multichannel=true`. Map `channel_index[0]` 0 to `you` and 1 to `them`. Keep final segments and endpoint state per channel. Start with `endpointing=300` milliseconds and make the model and language explicit. Verify current provider support and pricing before use.
- [ ] Provide direct OpenAI-compatible brain endpoints in Rust with a model and API key per provider. Support a Gemini AI Studio endpoint and other compatible providers. Test streaming and image capability against the selected provider. Report unsupported capabilities rather than silently changing providers.
- [ ] Provide local settings for adding, editing, testing, selecting, and removing providers. Show which transcriber and brain are active. Prevent listening or answering when the required configuration is missing. Explain that cloud providers receive audio or transcript data and may charge for use.
- [ ] Integrate the proven native capture into the session. Select the microphone and the PulseAudio or PipeWire monitor independently. Keep the sources on their assigned channels. Surface a missing source instead of silently labeling a monitor as `you`.
- [ ] Preserve and improve automatic answers. Default to question-triggered auto-answer for new configurations. Accumulate finalized `them` segments until the provider reports an endpoint, then use the brain model to decide whether a response is needed and stream the answer. Never auto-answer `you` speech. Preserve manual answer, cancellation, and visible error handling.
- [ ] Keep the active prompt, CV and job-description instructions, transcript, prior turns, meetings, and exports locally. Add in-app prompt editing and shortcut configuration for dashboard-only functions that remain useful. Preserve existing local meeting and settings data.
- [ ] Configure a new brain provider and verify its key works after restart without exposing it. If the OS keychain is unavailable, show a useful error without storing plaintext. Do not delete existing models, meeting data, or credentials from disk as a side effect.
- [ ] Remove account linking, cloud synchronization, dashboard actions, plan and quota checks, relay authentication and URLs, and their IPC and renderer surfaces. Disable the upstream auto-updater. Do not contact Meetingly services at runtime.
- [ ] Remove Nemotron, its worker, refinement pipeline, downloads, native vendor resources, packaging hooks, and settings. Trace callers before removing Electron, its updater, `koffi`, `eventsource`, `ws`, `openai`, obsolete Node scripts, and unused dependencies. Remove obsolete project code, not the user's Codex installation or unrelated files. Keep frontend JavaScript and TypeScript where the webview requires them.
- [ ] Keep failover explicit. Do not automatically retry an LLM request after text has appeared, because doing so can duplicate or alter an answer. On a transcription disconnect, show the interruption and reconnect the selected provider without inventing missing audio. Automatic cross-provider failover is not required for the first cutover.
- [ ] Replace the misleading `keepMeetingsLocal` behavior. Local persistence remains standard, while cloud transcription and model requests are clearly disclosed. No offline transcription fallback is included.

**You see.**

- [ ] A fresh user can configure and test a transcriber and brain without an account. A session shows separate `You` and `Them` turns and streams an answer to a question from `Them`.
- [ ] When a provider fails, the UI names the failing provider and preserves the meeting transcript already received.

**Verify, unit.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Port backend behavioral coverage to Rust tests for settings migration, secure key storage, channel parsing, per-channel endpointing, reconnect, and auto-answer. Retain focused Vitest cases for frontend behavior. Run `cd app/src-tauri && cargo test`, then `cd app && bun run --bun vitest run <test-files>`. At the final head run the full Rust and frontend suites, `bun run --bun typecheck`, and the Tauri production build.
- [ ] Test partial streaming failures, missing credentials, unavailable keychain, malformed provider messages, duplicate final events, and session stop with pending audio. Assert user-visible behavior and preserved data.
- [ ] Verify an interrupted migration can be retried without data loss. Verify rollback opens the original Electron data. Confirm the release package contains no Electron runtime, Node runtime, Bun runtime, migration helper, or obsolete cloud and ASR components.

**Verify, live.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked. Ten lanes on `auto` at the PR head, using isolated profiles where possible and the target Linux host for audio.

- [ ] Lane 1. Regression lane against trunk. Compare fresh startup and local transcript access on trunk and head. Save `/tmp/byok-1/startup.png`. Pass when the head opens without an account and preserves existing transcripts.
- [ ] Lane 2. Add and test a transcriber and a brain in the UI. Save `/tmp/byok-1/providers.png`. Pass when both configurations persist after restart without exposing keys.
- [ ] Lane 3. Run simultaneous scripted microphone and monitor input. Save `/tmp/byok-1/channels.png`. Pass when each line has the correct source label and both sources are audible to the app.
- [ ] Lane 4. Speak a question on the system channel. Save `/tmp/byok-1/answer.png`. Pass when a streamed answer appears without a manual action.
- [ ] Lane 5. Speak only on the microphone channel. Save `/tmp/byok-1/mic.png`. Pass when no auto-answer is triggered by the microphone utterance.
- [ ] Lane 6. Send a non-question in question mode. Save `/tmp/byok-1/non-question.png`. Pass when the configured model declines without a visible answer.
- [ ] Lane 7. Reject or disconnect a provider. Save `/tmp/byok-1/error.png`. Pass when the UI reports the failure and retains the existing transcript.
- [ ] Lane 8. Migrate a copy of existing settings and meetings. Save `/tmp/byok-1/migration.png`. Pass when the prior prompts, shortcuts, and meeting history remain usable. Legacy credentials are excluded from migration.
- [ ] Lane 9. Disable the Linux keychain. Save `/tmp/byok-1/keychain.png`. Pass when saving credentials fails clearly without plaintext storage or loss of the prior settings.
- [ ] Lane 10. Record application network connections through startup, listening, answering, and update checks. Save `/tmp/byok-1/network.png` and a sanitized connection log. Pass when no request reaches Meetingly account, relay, update, or model hosts.

**Verify, perf.** Tests alone are not sufficient verification. A PR is verified only when its unit, live, and perf boxes are all checked.

- [ ] Metric. Measure the interval from end of the spoken question to the first visible answer token. Also record speech end to final transcript and final transcript to first token.
- [ ] Probe. Run at least 20 representative questions on the same Linux machine and network. Record provider, model, language, audio device, settings, and failures. Compare trunk only if it can run the same provider and scenario. Record any unmatched baseline rather than claiming a direct comparison. Measure idle and active-session total process proportional set size and installed package size for Electron and Tauri on the same Linux host. Record webview and shared-library accounting, capture workload, and measurement commands.
- [ ] Baseline. Record trunk results first when comparable. Otherwise record that trunk lacks the direct BYOK transcription path and report head measurements alone. Report memory and package size without a preselected savings claim.
- [ ] Rule. Target a median below 3 seconds and a 95th percentile below 5 seconds for first visible answer text. These are acceptance targets, not measured guarantees. Report failures separately and investigate if the targets are missed.

**Preliminary ASR benchmark (not final perf acceptance).** Three realtime runs per provider used the same 66.024-second `input.mp3`, `transcription-reference.txt` (118 normalized words), English, and 100 ms chunks. Raw reports were removed after review; these preliminary figures remain in the plan. Median first-text latency from run start / after audio streaming began was 4.971 s / 4.541 s for Gemini Live (`gemini-3.5-transcribe-live`), 5.700 s / 4.793 s for Deepgram (`nova-3`), and 6.568 s / 4.842 s for OpenAI (`gpt-live-transcribe`). Mean WER was 13.56%, 12.71%, and 6.78%, respectively. OpenAI led on reference accuracy; Gemini Live was fastest. First-text timing includes speech content before the first provider text, not just provider processing. The single-clip ASR probe is not the required 20-question test and does not measure end-of-question to first answer token. OpenAI finalized about 0.6 s after the clip ended because this probe commits one whole-file turn; that finalization timing is not comparable to streaming utterance endpoints. Two `gemini-3.5-transcribe` batch attempts returned 503 and have no score. This comparison is research only; Deepgram remains the initial transcription implementation in scope.

**Review gate.** The operator reviews before merge.

- [ ] Give the operator a screenshot and a short video of provider setup and automatic answering, plus the sanitized network and latency results. Do not merge without a separate instruction.

**Merge.**

- [ ] Hand off the verified head for operator review. Merge only if the operator explicitly requests it.

## Close the program

- [ ] Complete the boxes with linked evidence. Record any failed or unavailable live checks without calling the rebuild complete.
- [ ] Report the supported provider types, migration and rollback results, latency and memory measurements, network evidence, toolchain versions, and remaining risks.

## Appendix A. Prototype evidence

The Tauri bootstrap and native Rust capture prototypes have been run. The results and remaining gaps are recorded in Implementation status. Webview compatibility, provider streaming, the full Bun-only toolchain, and target Linux packaging remain unproven.

## Appendix B. Alternatives rejected

Local Nemotron fallback conflicts with the requested removal of local transcription. A Groq Whisper fallback is not assumed to provide the same live streaming behavior as Deepgram. Automatic failover and multiple simultaneous transcript streams add loss, duplication, and ordering problems, so provider selection is explicit. Deepgram and OpenAI-compatible endpoints cover the initial transcriber and brain roles without implying that every provider has the same API.

A permanent Node sidecar conflicts with the native Rust target. Legacy credential migration is out of scope by operator request, so no Electron migration helper is needed. Bun runs the frontend tools, not the shipped backend. No speed advantage is assumed without measurement.

## Appendix C. Risks

Linux loopback and audio permissions vary by desktop and must be verified on the actual machine. Microphone bleed can misattribute system speech despite correct channel bookkeeping. Endpointing trades speed against splitting questions. Network latency, provider availability, model behavior, and rate limits affect answer speed. Gemini's OpenAI compatibility does not guarantee support for every OpenAI request parameter. A failed migration must preserve the old settings and meetings. API calls send sensitive audio and transcript content to the configured providers. Google lists `gemini-3.5-transcribe-live` as free on its Free Tier but publishes no fixed quota for it; limits are project-specific in AI Studio, and Free Tier data may be used to improve Google products. Do not promise unlimited free usage, and disclose this data-use difference before routing sensitive audio to the Free Tier.

Tauri on Linux uses WebKitGTK rather than Electron's Chromium. Browser media, shortcuts, and window behavior need target-host verification. New credentials require an available secure OS keychain. Bun can honor Node shebangs unless invoked with `--bun`, and some tooling may not support Bun. Do not claim a Node-free build until tested.

## Appendix D. Links and reading list

- [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) lists Rust and Fedora system dependencies.
- [Tauri existing project setup](https://v2.tauri.app/start/create-project/#manual-setup-tauri-cli) and [Vite integration](https://v2.tauri.app/start/frontend/vite/) cover the frontend and desktop build.
- [Tauri commands](https://v2.tauri.app/develop/calling-rust/) and [frontend channels](https://v2.tauri.app/develop/calling-frontend/) cover native IPC.
- [Bun runtime](https://bun.sh/docs/runtime) explains `bun run --bun` and shebang behavior.
- [Rust audio capture with cpal](https://docs.rs/cpal/latest/cpal/), [PipeWire bindings](https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/), and [PulseAudio bindings](https://docs.rs/libpulse-binding/latest/libpulse_binding/) are candidates for the Linux capture proof.
- [Rust keyring](https://docs.rs/crate/keyring/latest), [Tokio WebSockets](https://docs.rs/tokio-tungstenite/latest/tokio_tungstenite/), and [Tauri agent skills](https://github.com/full-stack-skills/tauri-skills) are starting points. Verify maintenance, versions, licensing, and security before adoption.
- [Deepgram multichannel](https://developers.deepgram.com/docs/multichannel) documents separate streaming channel indices.
- [Deepgram endpointing and interim results](https://developers.deepgram.com/docs/understand-endpointing-interim-results) distinguishes final segments from speech endpoints.
- [Deepgram live audio reference](https://developers.deepgram.com/reference/speech-to-text/listen-streaming) documents live WebSocket parameters.
- [Gemini OpenAI compatibility](https://ai.google.dev/gemini-api/docs/openai) documents the compatible endpoint and streaming calls.
- [Gemini Live transcription](https://ai.google.dev/gemini-api/docs/live-api/live-transcribe) documents the dedicated streaming ASR model.
- [Gemini API pricing](https://ai.google.dev/gemini-api/docs/pricing) and [rate limits](https://ai.google.dev/gemini-api/docs/rate-limits) document Free Tier pricing and project-specific quota behavior.
- `Updated fork build guide.md` contains the earlier plan. This plan supersedes its local-ASR fallback and Meetingly cloud assumptions.
