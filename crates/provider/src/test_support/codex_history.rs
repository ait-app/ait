use base64::Engine;
use serde_json::{Value, json};

use super::Fixture;

/// Write two newest-first pages with 130 messages and over 32 MiB of valid BMP images.
/// Returns the native JSON byte count; all fixture content is generated locally.
pub(crate) fn write_large_history(fixture: &Fixture) -> u64 {
    // A 512 x 512, 24-bit uncompressed BMP; rows are already aligned to four bytes.
    let mut bitmap = vec![127; 54 + 512 * 512 * 3];
    bitmap[..54].fill(0);
    let size = u32::try_from(bitmap.len()).unwrap();
    bitmap[..2].copy_from_slice(b"BM");
    bitmap[2..6].copy_from_slice(&size.to_le_bytes());
    bitmap[10..14].copy_from_slice(&54_u32.to_le_bytes());
    bitmap[14..18].copy_from_slice(&40_u32.to_le_bytes());
    bitmap[18..22].copy_from_slice(&512_u32.to_le_bytes());
    bitmap[22..26].copy_from_slice(&512_u32.to_le_bytes());
    bitmap[26..28].copy_from_slice(&1_u16.to_le_bytes());
    bitmap[28..30].copy_from_slice(&24_u16.to_le_bytes());
    let image = base64::engine::general_purpose::STANDARD.encode(bitmap);
    let mut data = vec![json!({"type":"turnStarted","position":0,
        "turnId":"previous-turn","startedAt":1_700_000_001})];
    for index in 1..=130 {
        let item = if index <= 34 {
            json!({"id":format!("item-{index}"),"type":"userMessage","content":[
                {"type":"image","url":format!("data:image/bmp;base64,{image}")}]})
        } else {
            json!({"id":format!("item-{index}"),"type":"agentMessage","text":format!("Reply {index}")})
        };
        data.push(json!({"type":"item","position":index,"turnId":"previous-turn","item":item}));
    }
    data.push(json!({"type":"turnCompleted","position":131,"turnId":"previous-turn"}));
    let older: Vec<Value> = data.drain(..32).collect();
    let pages = json!({"first":{"data":data,"nextCursor":"older"},
        "older":{"data":older,"nextCursor":null}});
    let path = fixture.cwd.join("timeline-pages.json");
    serde_json::to_writer(std::fs::File::create(&path).unwrap(), &pages).unwrap();
    std::fs::metadata(path).unwrap().len()
}
