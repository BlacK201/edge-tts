use anyhow::{anyhow, Result};
use rand::RngCore;
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};
use tungstenite::{Message, WebSocket};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use uuid::Uuid;
use xml::escape::{escape_str_attribute, escape_str_pcdata};


const SYNTH_URL: &str = "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken=6A5AA1D4EAFF4E9FB37E23D68491D6F4";

fn random_request_id() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(&buf[..])
}

fn parse_headers(s: impl AsRef<str>) -> Vec<(String, String)> {
    s.as_ref().split("\r\n").filter_map(|s| {
        if s.len() > 0 {
            let mut iter = s.splitn(2, ":");
            let k = iter.next().unwrap_or("").to_owned();
            let v = iter.next().unwrap_or("").to_owned();
            Some((k, v))
        } else {
            None
        }
    }).collect()
}

/// `voice_short_name`: eg: "zh-CN-XiaoxiaoNeural"
///
/// `pitch`
/// * x-low
/// * low
/// * medium
/// * high
/// * x-high
/// * default
///
/// `rate`
/// * x-slow
/// * slow
/// * medium
/// * fast
/// * x-fast
/// * default
///
/// `volume`
/// * silent
/// * x-soft
/// * soft
/// * medium
/// * loud
/// * x-loud
/// * default
pub fn build_ssml(text: &str, voice_short_name: &str, pitch: &str, rate: &str, volume: &str) -> String {
    format!("<speak version=\"1.0\" xmlns=\"http://www.w3.org/2001/10/synthesis\" xmlns:mstts=\"https://www.w3.org/2001/mstts\" xml:lang=\"en-US\"><voice name=\"{}\"><prosody pitch=\"{}\" rate=\"{}\" volume=\"{}\">{}</prosody></voice></speak>", escape_str_attribute(voice_short_name), escape_str_attribute(pitch), escape_str_attribute(rate), escape_str_attribute(volume), escape_str_pcdata(text))
}

pub fn configure_request(mut request: tungstenite::http::Request<()>) -> Result<tungstenite::http::Request<()>> {
    let headers = request.headers_mut();
    headers.insert(
        "Accept-Encoding",
        HeaderValue::from_static("gzip, deflate, br, zstd"),
    );
    headers.insert(
        "Accept-Language",
        HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8,en-GB;q=0.7,en-US;q=0.6"),
    );
    headers.insert(
        "User-Agent",
        HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0"),
    );
    headers.insert(
        "Origin",
        HeaderValue::from_static("chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold"),
    );
    Ok(request)
}
/// `output_format`: eg: "audio-24khz-48kbitrate-mono-mp3". See https://learn.microsoft.com/en-us/azure/ai-services/speech-service/rest-text-to-speech?tabs=streaming#audio-outputs
pub fn request_audio(ssml: &str, output_format: &str) -> anyhow::Result<Vec<u8>> {
    let synth_url = format!("{}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-143.0.3650.139&ConnectionId={}", SYNTH_URL, generate_sec_ms_gec_sync("6A5AA1D4EAFF4E9FB37E23D68491D6F4"), Uuid::new_v4());
    let request = synth_url.into_client_request()?;
    let request = configure_request(request)?;
    let (mut socket, _) = tungstenite::connect(request)?;
    process_socket_data(&ssml, &output_format, &mut socket)
}

/// `output_format`: eg: "audio-24khz-48kbitrate-mono-mp3". See https://learn.microsoft.com/en-us/azure/ai-services/speech-service/rest-text-to-speech?tabs=streaming#audio-outputs
/// `proxy_addr`: socks5 proxy addr，like "127.0.0.1:1080"
pub fn request_audio_via_socks5_proxy(ssml: &str, output_format: &str, proxy_addr: &str) -> anyhow::Result<Vec<u8>> {
    let synth_url = format!("{}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-143.0.3650.139&ConnectionId={}", SYNTH_URL, generate_sec_ms_gec_sync("6A5AA1D4EAFF4E9FB37E23D68491D6F4"), Uuid::new_v4());
    let url = url::Url::parse(&synth_url)?;
    let host = url.host_str().unwrap();
    let port = url.port_or_known_default().unwrap();

    let proxy_stream = socks::Socks5Stream::connect(proxy_addr, (host, port))?;
    let tls_connector = native_tls::TlsConnector::new()?;
    let tls_stream = tls_connector.connect(host, proxy_stream)?;
    let request = url.into_client_request()?;
    let request = configure_request(request)?;
    let (mut socket, _) = tungstenite::client::client(request, tls_stream)?;
    process_socket_data(&ssml, &output_format, &mut socket)
}

fn generate_sec_ms_gec_sync(trusted_client_token: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let ticks = now + 11644473600;
    let rounded = ticks - (ticks % 300);
    let windows_ticks = rounded * 10000000;

    let data = format!("{}{}", windows_ticks, trusted_client_token);

    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    let hash_result = hasher.finalize();

    hash_result
        .iter()
        .map(|byte| format!("{:02X}", byte))
        .collect::<String>()
}
fn process_socket_data<S: std::io::Read + std::io::Write>(
    ssml: &str,
    output_format: &str,
    socket: &mut WebSocket<S>,
) -> Result<Vec<u8>> {
    socket.send(Message::Text(format!("Content-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":false,\"wordBoundaryEnabled\":true}},\"outputFormat\":\"{}\"}}}}}}}}", output_format)))?;
    let request_id = random_request_id();
    socket.send(Message::Text(format!("X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nPath:ssml\r\n\r\n{}", request_id, ssml)))?;
    let mut buf = Vec::new();
    loop {
        match socket.read() {
            Ok(msg) => {
                match msg {
                    Message::Text(s) => {
                        if let Some(header_str) = s.splitn(2, "\r\n\r\n").next() {
                            let headers = parse_headers(header_str);
                            if headers.iter().any(|(k, v)| k == "Path" && v == "turn.end") {
                                if headers.iter().any(|(k, v)| k == "X-RequestId" && v.as_str() == request_id) {
                                    return Ok(buf);
                                } else {
                                    return Err(anyhow!("Path:turn.end no X-RequestId header"));
                                }
                            }
                        } else {
                            return Err(anyhow!("bad text response. message not complete"));
                        }
                    }
                    Message::Binary(s) => {
                        let header_len = s[0] as usize * 256 + s[1] as usize;
                        if s.len() >= header_len + 2 {
                            let headers = parse_headers(String::from_utf8_lossy(&s[2..header_len]));
                            let body = &s[(header_len + 2)..];
                            if headers.iter().any(|(k, v)| k == "Path" && v == "audio") {
                                if headers.iter().any(|(k, v)| k == "X-RequestId" && v.as_str() == request_id) {
                                    buf.extend(body);
                                } else {
                                    return Err(anyhow!("Path:audio no X-RequestId header"));
                                }
                            }
                        } else {
                            return Err(anyhow!("bad binary response. response len: {} header len: {}", s.len(), header_len));
                        }
                    }
                    _ => {}
                };
            }
            Err(e) => {
                return Err(anyhow!("socket read error: {:?}", e));
            }
        };
    }
}
