//! Direct Deepgram multichannel streaming WebSocket client.

use crate::meetings::Channel;
use crate::settings::ProviderRole;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use url::Url;

const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(8);
const OPEN_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEvent {
    pub text: String,
    pub is_final: bool,
    pub speech_final: bool,
    pub confidence: f64,
    pub speaker: Option<i64>,
    pub channel: Option<Channel>,
}

#[derive(Debug, Clone)]
pub struct DeepgramConfig {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub language: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub endpointing_ms: u32,
}

impl DeepgramConfig {
    pub fn multichannel(endpoint: String, api_key: String, model: String, language: String) -> Self {
        Self {
            endpoint,
            api_key,
            model,
            language,
            sample_rate: 16_000,
            channels: 2,
            endpointing_ms: 300,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SocketEvent {
    Transcript(TranscriptEvent),
    Error(String),
    Closed { by_us: bool, reason: String },
}

enum SocketCommand {
    Audio(Vec<u8>),
    Close,
}

#[derive(Clone)]
pub struct DeepgramHandle {
    tx: mpsc::Sender<SocketCommand>,
}

impl DeepgramHandle {
    pub fn send_audio(&self, pcm16: Vec<u8>) -> bool {
        self.tx.try_send(SocketCommand::Audio(pcm16)).is_ok()
    }

    pub fn close(&self) {
        let _ = self.tx.try_send(SocketCommand::Close);
    }
}

pub fn build_listen_url(config: &DeepgramConfig) -> Result<Url, String> {
    let mut url = Url::parse(&config.endpoint).map_err(|e| format!("Invalid Deepgram URL: {e}"))?;
    {
        let mut query = url.query_pairs_mut();
        query.clear();
        if !config.model.trim().is_empty() {
            query.append_pair("model", config.model.trim());
        }
        if !config.language.trim().is_empty() {
            query.append_pair("language", config.language.trim());
        }
        query
            .append_pair("encoding", "linear16")
            .append_pair("sample_rate", &config.sample_rate.to_string())
            .append_pair("channels", &config.channels.to_string())
            .append_pair("interim_results", "true")
            .append_pair("punctuate", "true")
            .append_pair("smart_format", "true")
            .append_pair("endpointing", &config.endpointing_ms.to_string())
            .append_pair("vad_events", "true")
            .append_pair("diarize", "true");
        if config.channels > 1 {
            query.append_pair("multichannel", "true");
        }
    }
    Ok(url)
}

pub async fn connect(config: &DeepgramConfig) -> Result<(DeepgramHandle, mpsc::UnboundedReceiver<SocketEvent>), String> {
    let url = build_listen_url(config)?;
    let mut request = url.as_str().into_client_request().map_err(|e| format!("Invalid Deepgram request: {e}"))?;
    let auth = format!("Token {}", config.api_key);
    request
        .headers_mut()
        .insert("Authorization", auth.parse().map_err(|_| "Deepgram API key contains invalid header characters.".to_string())?);

    let (ws, _) = tokio::time::timeout(OPEN_TIMEOUT, tokio_tungstenite::connect_async(request))
        .await
        .map_err(|_| "Deepgram did not answer in time.".to_string())?
        .map_err(|e| describe_ws_error(&e))?;

    let (mut sink, mut stream) = ws.split();
    let (cmd_tx, mut cmd_rx) = mpsc::channel::<SocketCommand>(64);
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        let mut keep_alive = tokio::time::interval(KEEP_ALIVE_INTERVAL);
        keep_alive.tick().await;
        let mut by_us = false;
        let mut reason = String::new();

        loop {
            tokio::select! {
                _ = keep_alive.tick() => {
                    if sink.send(Message::Text(r#"{"type":"KeepAlive"}"#.into())).await.is_err() {
                        break;
                    }
                }
                cmd = cmd_rx.recv() => {
                    match cmd {
                        Some(SocketCommand::Audio(bytes)) => {
                            if sink.send(Message::Binary(bytes.into())).await.is_err() {
                                break;
                            }
                        }
                        Some(SocketCommand::Close) | None => {
                            by_us = true;
                            let _ = sink.send(Message::Text(r#"{"type":"CloseStream"}"#.into())).await;
                            let _ = sink.close().await;
                            break;
                        }
                    }
                }
                msg = stream.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            if let Some(event) = parse_deepgram_message(&text) {
                                let _ = evt_tx.send(SocketEvent::Transcript(event));
                            }
                        }
                        Some(Ok(Message::Close(frame))) => {
                            if let Some(f) = frame {
                                reason = f.reason.to_string();
                            }
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(err)) => {
                            let _ = evt_tx.send(SocketEvent::Error(describe_ws_error(&err)));
                            break;
                        }
                        None => break,
                    }
                }
            }
        }

        let _ = evt_tx.send(SocketEvent::Closed { by_us, reason });
    });

    Ok((DeepgramHandle { tx: cmd_tx }, evt_rx))
}

pub async fn test_connection(endpoint: &str, api_key: &str, model: &str, language: &str) -> crate::llm::ProviderTestResult {
    let config = DeepgramConfig::multichannel(endpoint.to_string(), api_key.to_string(), model.to_string(), language.to_string());
    match connect(&config).await {
        Ok((handle, _rx)) => {
            handle.close();
            crate::llm::ProviderTestResult {
                ok: true,
                role: ProviderRole::Transcription,
                streaming: Some(true),
                vision: None,
                detail: "Deepgram live streaming connection succeeded.".into(),
            }
        }
        Err(detail) => crate::llm::ProviderTestResult {
            ok: false,
            role: ProviderRole::Transcription,
            streaming: Some(false),
            vision: None,
            detail,
        },
    }
}

fn describe_ws_error(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Http(res) => match res.status().as_u16() {
            401 | 403 => "Deepgram rejected the API key. Check the key and try again.".into(),
            404 => "Deepgram endpoint returned 404. Check the WebSocket URL.".into(),
            429 => "Deepgram rate limit or quota exceeded.".into(),
            code => format!("Deepgram refused the connection (HTTP {code})."),
        },
        Error::Io(io) => format!("Cannot reach Deepgram: {io}"),
        other => format!("Deepgram connection error: {other}"),
    }
}

#[derive(Deserialize)]
struct RawResults {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    is_final: bool,
    #[serde(default)]
    speech_final: bool,
    #[serde(default)]
    channel_index: Vec<u16>,
    channel: Option<RawChannel>,
}

#[derive(Deserialize)]
struct RawChannel {
    #[serde(default)]
    alternatives: Vec<RawAlternative>,
}

#[derive(Deserialize)]
struct RawAlternative {
    transcript: Option<String>,
    #[serde(default)]
    confidence: f64,
    #[serde(default)]
    words: Vec<RawWord>,
}

#[derive(Deserialize)]
struct RawWord {
    speaker: Option<i64>,
}

pub fn parse_deepgram_message(raw: &str) -> Option<TranscriptEvent> {
    let msg: RawResults = serde_json::from_str(raw).ok()?;
    if msg.kind.as_deref() != Some("Results") {
        return None;
    }
    let alt = msg.channel?.alternatives.into_iter().next()?;
    let text = alt.transcript?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let channel = match msg.channel_index.first().copied() {
        Some(0) => Some(Channel::You),
        Some(1) => Some(Channel::Them),
        _ => None,
    };
    let speaker = alt.words.into_iter().find_map(|w| w.speaker);
    Some(TranscriptEvent {
        text,
        is_final: msg.is_final,
        speech_final: msg.speech_final,
        confidence: alt.confidence,
        speaker,
        channel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn builds_multichannel_listen_url() {
        let cfg = DeepgramConfig::multichannel(
            "wss://api.deepgram.com/v1/listen".into(),
            "dg-secret".into(),
            "nova-3".into(),
            "en".into(),
        );
        let url = build_listen_url(&cfg).unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs.get("model").map(String::as_str), Some("nova-3"));
        assert_eq!(pairs.get("language").map(String::as_str), Some("en"));
        assert_eq!(pairs.get("encoding").map(String::as_str), Some("linear16"));
        assert_eq!(pairs.get("sample_rate").map(String::as_str), Some("16000"));
        assert_eq!(pairs.get("channels").map(String::as_str), Some("2"));
        assert_eq!(pairs.get("multichannel").map(String::as_str), Some("true"));
        assert_eq!(pairs.get("endpointing").map(String::as_str), Some("300"));
    }

    #[test]
    fn parses_channel_index_0_as_you_and_1_as_them() {
        let you_msg = r#"{
            "type": "Results",
            "channel_index": [0, 2],
            "is_final": true,
            "speech_final": false,
            "channel": { "alternatives": [{ "transcript": "  I built that service ", "confidence": 0.96, "words": [{"speaker": 0}] }] }
        }"#;
        let them_msg = r#"{
            "type": "Results",
            "channel_index": [1, 2],
            "is_final": true,
            "speech_final": true,
            "channel": { "alternatives": [{ "transcript": "Tell me about Kafka.", "confidence": 0.91 }] }
        }"#;
        assert_eq!(
            parse_deepgram_message(you_msg),
            Some(TranscriptEvent {
                text: "I built that service".into(),
                is_final: true,
                speech_final: false,
                confidence: 0.96,
                speaker: Some(0),
                channel: Some(Channel::You),
            })
        );
        assert_eq!(
            parse_deepgram_message(them_msg),
            Some(TranscriptEvent {
                text: "Tell me about Kafka.".into(),
                is_final: true,
                speech_final: true,
                confidence: 0.91,
                speaker: None,
                channel: Some(Channel::Them),
            })
        );
    }

    #[test]
    fn ignores_metadata_empty_transcripts_and_malformed_json() {
        assert_eq!(parse_deepgram_message(r#"{"type":"Metadata"}"#), None);
        assert_eq!(
            parse_deepgram_message(r#"{"type":"Results","channel":{"alternatives":[{"transcript":"   "}]}}"#),
            None
        );
        assert_eq!(parse_deepgram_message("not json"), None);
    }

    #[tokio::test]
    async fn streams_audio_and_receives_multichannel_transcripts_over_websocket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut seen_auth = None;
            let ws = tokio_tungstenite::accept_hdr_async(
                stream,
                |req: &tokio_tungstenite::tungstenite::handshake::server::Request, res| {
                    seen_auth = req.headers().get("Authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
                    Ok(res)
                },
            )
            .await
            .unwrap();
            assert_eq!(seen_auth.as_deref(), Some("Token dg-test-key"));
            let (mut sink, mut rx) = ws.split();
            let audio = rx.next().await.unwrap().unwrap();
            assert_eq!(audio.into_data().as_ref(), &[1u8, 2, 3, 4]);
            sink.send(Message::Text(
                r#"{"type":"Results","channel_index":[1,2],"is_final":true,"speech_final":true,"channel":{"alternatives":[{"transcript":"What is Raft?","confidence":0.99}]}}"#.into(),
            ))
            .await
            .unwrap();
            let close = rx.next().await.unwrap().unwrap();
            assert_eq!(close.to_text().unwrap(), r#"{"type":"CloseStream"}"#);
        });

        let cfg = DeepgramConfig::multichannel(
            format!("ws://{addr}/v1/listen"),
            "dg-test-key".into(),
            "nova-3".into(),
            "en".into(),
        );
        let (handle, mut events) = connect(&cfg).await.unwrap();
        assert!(handle.send_audio(vec![1, 2, 3, 4]));
        let first = events.recv().await.unwrap();
        match first {
            SocketEvent::Transcript(ev) => {
                assert_eq!(ev.text, "What is Raft?");
                assert_eq!(ev.channel, Some(Channel::Them));
                assert!(ev.is_final && ev.speech_final);
            }
            other => panic!("unexpected event: {other:?}"),
        }
        handle.close();
        let closed = events.recv().await.unwrap();
        assert!(matches!(closed, SocketEvent::Closed { by_us: true, .. }));
        server.await.unwrap();
    }
}
