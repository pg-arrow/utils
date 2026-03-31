use core::slice;
use std::alloc::Layout;
use std::fmt::Write;

use pg_arrow::file::{
    HeapPageData, HeapTupleData, InfoMask, LP_DEAD, LP_NORMAL, LP_REDIRECT, LP_UNUSED,
    PAGE_BUFFER_SIZE, HEAP_NATTS_MASK, SIZEOF_HEAP_TUPLE_HEADER,
};

// --- pd_flags bits ---
const PD_HAS_FREE_LINES: u16 = 0x0001;
const PD_PAGE_FULL: u16 = 0x0002;
const PD_ALL_VISIBLE: u16 = 0x0004;

// --- t_infomask2 flag bits (above HEAP_NATTS_MASK) ---
const HEAP_KEYS_UPDATED: u16 = 0x2000;
const HEAP_HOT_UPDATED: u16 = 0x4000;
const HEAP_ONLY_TUPLE: u16 = 0x8000;

fn lp_flags_name(flags: u8) -> &'static str {
    match flags {
        LP_UNUSED => "UNUSED",
        LP_NORMAL => "NORMAL",
        LP_REDIRECT => "REDIRECT",
        LP_DEAD => "DEAD",
        _ => "UNKNOWN",
    }
}

fn decode_pd_flags(flags: u16) -> String {
    let mut parts = Vec::new();
    if flags & PD_HAS_FREE_LINES != 0 {
        parts.push("HAS_FREE_LINES");
    }
    if flags & PD_PAGE_FULL != 0 {
        parts.push("PAGE_FULL");
    }
    if flags & PD_ALL_VISIBLE != 0 {
        parts.push("ALL_VISIBLE");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(" | ")
    }
}

fn decode_infomask(mask: u16) -> String {
    const ALL_FLAGS: &[InfoMask] = &[
        InfoMask::HasNull,
        InfoMask::HasVarWidth,
        InfoMask::HasExternal,
        InfoMask::HasOidOld,
        InfoMask::XmaxKeyshrLock,
        InfoMask::ComboCid,
        InfoMask::XmaxExclLock,
        InfoMask::XmaxLockOnly,
        InfoMask::XminCommitted,
        InfoMask::XminInvalid,
        InfoMask::XmaxCommitted,
        InfoMask::XmaxInvalid,
        InfoMask::XmaxIsMulti,
        InfoMask::Updated,
        InfoMask::MovedOff,
        InfoMask::MovedIn,
    ];
    let set: Vec<&str> = ALL_FLAGS
        .iter()
        .filter(|&&flag| mask & (flag as u16) != 0)
        .map(|flag| flag.short_name())
        .collect();
    if set.is_empty() {
        "none".to_string()
    } else {
        set.join(" | ")
    }
}

fn decode_infomask2(mask: u16) -> String {
    let mut parts = Vec::new();
    if mask & HEAP_KEYS_UPDATED != 0 {
        parts.push("KEYS_UPDATED");
    }
    if mask & HEAP_HOT_UPDATED != 0 {
        parts.push("HOT_UPDATED");
    }
    if mask & HEAP_ONLY_TUPLE != 0 {
        parts.push("HEAP_ONLY");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(" | ")
    }
}

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out
}

struct JsonArray {
    buf: String,
    first: bool,
}

impl JsonArray {
    fn new() -> Self {
        Self {
            buf: String::from("["),
            first: true,
        }
    }

    fn entry(&mut self, label: &str, value: &str, offset: usize, length: usize) {
        if !self.first {
            self.buf.push(',');
        }
        self.first = false;
        write!(
            self.buf,
            r#"{{"label":"{}","value":"{}","offset":{},"length":{}}}"#,
            json_escape(label),
            json_escape(value),
            offset,
            length
        )
        .unwrap();
    }

    fn separator(&mut self, label: &str) {
        if !self.first {
            self.buf.push(',');
        }
        self.first = false;
        write!(
            self.buf,
            r#"{{"label":"{}","value":""}}"#,
            json_escape(label),
        )
        .unwrap();
    }

    fn finish(mut self) -> String {
        self.buf.push(']');
        self.buf.push('\0');
        self.buf
    }
}

/// Emit one tuple header into the JSON array.
fn emit_tuple(j: &mut JsonArray, i: usize, tuple: &HeapTupleData, tuple_off: usize) {
    let th = &tuple.header;

    j.separator(&format!("=== Tuple {} Header (23 B) ===", i));

    j.entry("t_xmin", &format!("{}", th.t_xmin), tuple_off, 4);
    j.entry("t_xmax", &format!("{}", th.t_xmax), tuple_off + 4, 4);
    j.entry("t_cid/t_xvac", &format!("{}", th.t_field3), tuple_off + 8, 4);

    let block_num = th.t_ctid.ip_blkid.block_number();
    j.entry(
        "t_ctid",
        &format!("({}, {})", block_num, th.t_ctid.ip_posid),
        tuple_off + 12,
        6,
    );

    let natts = th.t_infomask2 & HEAP_NATTS_MASK;
    j.entry(
        "t_infomask2",
        &format!(
            "0x{:04X} natts={} [{}]",
            th.t_infomask2,
            natts,
            decode_infomask2(th.t_infomask2)
        ),
        tuple_off + 18,
        2,
    );

    j.entry(
        "t_infomask",
        &format!(
            "0x{:04X} [{}]",
            th.t_infomask,
            decode_infomask(th.t_infomask)
        ),
        tuple_off + 20,
        2,
    );

    j.entry(
        "t_hoff",
        &format!("{} (data starts at tuple+{})", th.t_hoff, th.t_hoff),
        tuple_off + 22,
        1,
    );

    // Null bitmap
    if th.has_flag(InfoMask::HasNull) && !tuple.null_bitmap.is_empty() {
        let bitmap_bytes = tuple.null_bitmap.len();
        let bm_start = tuple_off + SIZEOF_HEAP_TUPLE_HEADER;
        let mut null_cols = Vec::new();
        for col in 0..natts as usize {
            if col / 8 < bitmap_bytes && tuple.null_bitmap[col / 8] & (1 << (col % 8)) == 0 {
                null_cols.push(col);
            }
        }
        let desc = if null_cols.is_empty() {
            "all NOT NULL".to_string()
        } else {
            format!("NULLs at cols {:?}", null_cols)
        };
        j.entry("null_bitmap", &desc, bm_start, bitmap_bytes);
    }

    let data_len = tuple.data.len();
    if data_len > 0 {
        j.entry(
            "tuple_data",
            &format!("{} B", data_len),
            tuple_off + th.t_hoff as usize,
            data_len,
        );
    }
}

/// Minimal JSON string value extractor: find `"key":"value"` and return value.
/// No external deps — just enough to parse `{"all_rows":"true"}`.
fn json_get_str<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    // Look for "key" followed by : and "value"
    let needle = format!("\"{}\"", key);
    let key_pos = json.find(&needle)?;
    let after_key = &json[key_pos + needle.len()..];
    // Skip whitespace and colon
    let after_colon = after_key.trim_start().strip_prefix(':')?;
    let after_ws = after_colon.trim_start();
    // Extract quoted value
    let after_quote = after_ws.strip_prefix('"')?;
    let end = after_quote.find('"')?;
    Some(&after_quote[..end])
}

fn decode_page(bytes: &[u8], all_rows: bool) -> String {
    let mut j = JsonArray::new();

    if bytes.len() < PAGE_BUFFER_SIZE {
        j.entry(
            "Error",
            &format!(
                "Need full {} B page, got {} B",
                PAGE_BUFFER_SIZE,
                bytes.len()
            ),
            0,
            bytes.len(),
        );
        return j.finish();
    }

    // Copy into fixed-size buffer for HeapPageData::parse
    let mut page_buf = [0u8; PAGE_BUFFER_SIZE];
    page_buf.copy_from_slice(&bytes[..PAGE_BUFFER_SIZE]);

    let page = match HeapPageData::parse(page_buf) {
        Ok(p) => p,
        Err(e) => {
            j.entry("Error", &format!("{}", e), 0, PAGE_BUFFER_SIZE);
            return j.finish();
        }
    };

    let hdr = &page.header;

    // --- PageHeaderData (24 bytes) ---
    j.separator("=== Page Header (24 B) ===");

    j.entry(
        "pd_lsn",
        &format!("{:X}/{:08X}", hdr.pd_lsn.xlogid, hdr.pd_lsn.xrecoff),
        0,
        8,
    );

    j.entry(
        "pd_checksum",
        &format!("0x{:04X} ({})", hdr.pd_checksum, hdr.pd_checksum),
        8,
        2,
    );

    j.entry(
        "pd_flags",
        &format!("0x{:04X} [{}]", hdr.pd_flags, decode_pd_flags(hdr.pd_flags)),
        10,
        2,
    );

    j.entry("pd_lower", &format!("{}", hdr.pd_lower), 12, 2);
    j.entry("pd_upper", &format!("{}", hdr.pd_upper), 14, 2);
    j.entry("pd_special", &format!("{}", hdr.pd_special), 16, 2);

    j.entry(
        "pd_pagesize_version",
        &format!("size={} ver={}", hdr.page_size(), hdr.page_version()),
        18,
        2,
    );

    j.entry("pd_prune_xid", &format!("{}", hdr.pd_prune_xid), 20, 4);

    // Summary
    let free_space = if hdr.pd_upper >= hdr.pd_lower {
        hdr.pd_upper - hdr.pd_lower
    } else {
        0
    };
    j.separator(&format!(
        "--- {} line ptrs, {} B free ---",
        page.lp_num, free_space
    ));

    // --- Line pointers ---
    let max_lp = if all_rows {
        page.lp_num
    } else {
        std::cmp::min(page.lp_num, 32)
    };
    for i in 0..max_lp {
        let lp = &page.lp_items[i];
        let off = 24 + i * 4;
        j.entry(
            &format!("lp[{}]", i),
            &format!(
                "off={} flags={} len={}",
                lp.lp_off(),
                lp_flags_name(lp.lp_flags()),
                lp.lp_len()
            ),
            off,
            4,
        );
    }
    if page.lp_num > max_lp {
        j.separator(&format!("... +{} more line pointers", page.lp_num - max_lp));
    }

    // --- Decode tuple headers ---
    for (i, lp) in page.lp_items.iter().enumerate() {
        if lp.lp_flags() != LP_NORMAL {
            continue;
        }

        let tuple_off = lp.lp_off() as usize;
        let tuple_len = lp.lp_len() as usize;
        let end = tuple_off + tuple_len;
        if end > PAGE_BUFFER_SIZE || tuple_len < SIZEOF_HEAP_TUPLE_HEADER {
            continue;
        }

        let raw = &page.page_data[tuple_off..end];
        match HeapTupleData::parse_and_build(raw) {
            Ok(tuple) => emit_tuple(&mut j, i, &tuple, tuple_off),
            Err(e) => {
                j.entry(
                    &format!("Tuple {} parse error", i),
                    &format!("{}", e),
                    tuple_off,
                    tuple_len,
                );
            }
        }

        if !all_rows {
            break;
        }
    }

    j.finish()
}

// --- WASM ABI ---

fn return_json(json: String) -> i32 {
    let raw = json.into_bytes();
    let ptr = raw.as_ptr() as i32;
    std::mem::forget(raw);
    ptr
}

#[unsafe(no_mangle)]
pub extern "C" fn alloc(size: i32) -> i32 {
    let layout = Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { std::alloc::alloc(layout) as i32 }
}

/// Return parameter definitions for the decoder settings UI.
#[unsafe(no_mangle)]
pub extern "C" fn params() -> i32 {
    let json = r#"[{"name":"all_rows","type":"bool","default":"false"}]"#;
    let mut buf = json.to_string();
    buf.push('\0');
    return_json(buf)
}

/// Decode without params (default: show first tuple only).
#[unsafe(no_mangle)]
pub extern "C" fn decode(ptr: i32, len: i32, _endian: i32) -> i32 {
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    return_json(decode_page(bytes, false))
}

/// Decode with user-configured params.
#[unsafe(no_mangle)]
pub extern "C" fn decode_with_params(
    ptr: i32,
    len: i32,
    _endian: i32,
    params_ptr: i32,
    params_len: i32,
) -> i32 {
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len as usize) };
    let params_bytes =
        unsafe { slice::from_raw_parts(params_ptr as *const u8, params_len as usize) };
    let params_str = std::str::from_utf8(params_bytes).unwrap_or("");

    let all_rows = json_get_str(params_str, "all_rows") == Some("true");

    return_json(decode_page(bytes, all_rows))
}
