//! Text extraction for course materials and submitted files, so the AI can read them.
//!
//! Supported: plain text and source code (UTF-8 or CP949), PDF, Word (.docx), PowerPoint (.pptx),
//! Excel (.xlsx), Hangul (.hwpx and .hwp 5.x), Jupyter notebooks and ZIP archives of the above.
//! Everything runs in memory; nothing is written to disk.
use std::io::{Cursor, Read};

/// Largest file we try to read into memory.
pub const MAX_READ_BYTES: u64 = 60 * 1024 * 1024;
/// Largest text returned in one call (the caller pages with `offset`).
pub const PAGE_CHARS: usize = 25_000;

const TEXT_EXTS: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "xml", "html", "htm", "css", "scss", "yaml", "yml", "toml", "ini", "cfg", "conf", "log",
    "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "cs", "java", "kt", "kts", "scala", "go", "rs", "py", "pyw", "rb", "php", "pl", "swift", "m", "mm",
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "vue", "svelte", "sql", "sh", "bash", "zsh", "ps1", "bat", "cmd", "r", "jl", "lua", "dart", "asm", "s",
    "v", "sv", "vhd", "vhdl", "hs", "ml", "ex", "exs", "erl", "clj", "lisp", "scm", "f", "f90", "pas", "tex", "bib", "gradle", "makefile", "cmake",
    "dockerfile", "gitignore", "env", "properties", "srt", "vtt", "rtf",
];
const BINARY_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "svg", "ico", "tif", "tiff", "heic", "mp3", "wav", "m4a", "aac", "ogg", "flac", "mp4", "mov", "avi",
    "mkv", "webm", "wmv", "exe", "dll", "so", "dylib", "class", "jar", "o", "obj", "pyc", "7z", "rar", "gz", "tar", "bz2", "xz", "iso", "dmg", "doc",
    "ppt", "xls",
];

pub fn extension(name: &str) -> String {
    let lower = name.to_lowercase();
    let base = lower.rsplit(['/', '\\']).next().unwrap_or(&lower);
    if matches!(base, "makefile" | "dockerfile" | "cmakelists.txt") {
        return base.split('.').next().unwrap_or(base).to_string();
    }
    base.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or_default()
}

/// Extracts readable text from a file's bytes. `name` is used for the format.
pub fn extract_text(name: &str, bytes: &[u8]) -> Result<String, String> {
    let ext = extension(name);
    let is_zip = bytes.starts_with(b"PK\x03\x04");
    let text = match ext.as_str() {
        "pdf" => pdf(bytes)?,
        "docx" | "docm" | "dotx" if is_zip => docx(bytes)?,
        "pptx" | "pptm" | "ppsx" if is_zip => pptx(bytes)?,
        "xlsx" | "xlsm" if is_zip => xlsx(bytes)?,
        "hwpx" if is_zip => hwpx(bytes)?,
        "hwp" => hwp(bytes)?,
        "ipynb" => notebook(bytes)?,
        "zip" => archive(bytes)?,
        "doc" | "ppt" | "xls" => return Err(format!("옛 Office 형식(.{ext})은 읽을 수 없습니다. 파일을 .{ext}x로 저장한 자료가 있는지 확인하세요.")),
        e if BINARY_EXTS.contains(&e) => return Err(format!("'{name}'은(는) 텍스트가 없는 형식(.{e})이라 내용을 읽을 수 없습니다.")),
        _ if bytes.starts_with(b"%PDF") => pdf(bytes)?,
        _ if is_zip => archive(bytes)?,
        _ => decode_text(bytes).ok_or_else(|| format!("'{name}'은(는) 텍스트로 읽을 수 없는 형식입니다."))?,
    };
    let text = tidy(&text);
    if text.trim().is_empty() {
        return Err(format!("'{name}'에서 텍스트를 찾지 못했습니다 (스캔 이미지 PDF이거나 내용이 비어 있을 수 있음)."));
    }
    Ok(text)
}

/// Returns one page of `text` starting at char `offset`, with a note on how to continue.
pub fn page(text: &str, offset: usize, limit: usize) -> (String, Option<usize>, usize) {
    let total = text.chars().count();
    let chunk: String = text.chars().skip(offset).take(limit).collect();
    let next = if offset + limit < total { Some(offset + limit) } else { None };
    (chunk, next, total)
}

fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.replace("\r\n", "\n").replace('\r', "\n").lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

/// UTF-8 (with or without BOM), UTF-16 with BOM, or CP949/EUC-KR as used by Korean Windows editors.
pub fn decode_text(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some(String::from_utf8_lossy(rest).into_owned());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let (text, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        return Some(text.into_owned());
    }
    let sample = &bytes[..bytes.len().min(8192)];
    let nul = sample.iter().filter(|b| **b == 0).count();
    if nul > sample.len() / 100 + 1 {
        return None;
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Some(s.to_string());
    }
    let (text, _, had_errors) = encoding_rs::EUC_KR.decode(bytes);
    if !had_errors {
        return Some(text.into_owned());
    }
    let lossy = String::from_utf8_lossy(bytes);
    let bad = lossy.chars().filter(|c| *c == '\u{FFFD}').count();
    (bad * 50 < lossy.chars().count()).then(|| lossy.into_owned())
}

fn pdf(bytes: &[u8]) -> Result<String, String> {
    let owned = bytes.to_vec();
    match std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&owned)) {
        Ok(Ok(text)) => Ok(text),
        Ok(Err(e)) => Err(format!("PDF를 읽지 못했습니다: {e}")),
        Err(_) => Err("PDF 구조를 해석하지 못했습니다 (암호가 걸렸거나 손상된 파일일 수 있음).".into()),
    }
}

fn open_zip(bytes: &[u8]) -> Result<zip::ZipArchive<Cursor<&[u8]>>, String> {
    zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("압축 구조를 읽지 못했습니다: {e}"))
}

fn zip_entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let entry = zip.by_name(name).ok()?;
    if entry.size() > limit {
        return None;
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(limit).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn zip_names(zip: &zip::ZipArchive<Cursor<&[u8]>>) -> Vec<String> {
    zip.file_names().map(str::to_string).collect()
}

/// Sorts names like slide2.xml before slide10.xml.
fn numeric_sort(names: &mut [String]) {
    names.sort_by_key(|n| {
        let digits: String = n.rsplit('/').next().unwrap_or(n).chars().filter(char::is_ascii_digit).collect();
        (digits.parse::<u64>().unwrap_or(0), n.clone())
    });
}

/// Minimal XML text extraction: text inside `text_tags`, a newline after `break_tags`, a tab for `tab_tags`.
fn xml_text(xml: &str, text_tags: &[&str], break_tags: &[&str], tab_tags: &[&str]) -> String {
    let mut out = String::new();
    let mut inside_text = 0usize;
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if inside_text > 0 {
            out.push_str(&decode_entities(&rest[..lt]));
        }
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if text_tags.contains(&name) {
            if closing {
                inside_text = inside_text.saturating_sub(1);
            } else if !self_closing {
                inside_text += 1;
            }
        }
        if closing && break_tags.contains(&name) {
            out.push('\n');
        }
        if !closing && (tab_tags.contains(&name) || (self_closing && break_tags.contains(&name))) {
            out.push(if tab_tags.contains(&name) { '\t' } else { '\n' });
        }
    }
    out
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        let Some(semi) = after.find(';').filter(|i| *i <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let entity = &after[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn docx(bytes: &[u8]) -> Result<String, String> {
    let mut zip = open_zip(bytes)?;
    let xml = zip_entry(&mut zip, "word/document.xml", 50 << 20).ok_or("Word 문서 본문을 찾지 못했습니다.")?;
    Ok(xml_text(&String::from_utf8_lossy(&xml), &["w:t"], &["w:p", "w:br", "w:tr"], &["w:tab", "w:tc"]))
}

fn pptx(bytes: &[u8]) -> Result<String, String> {
    let mut zip = open_zip(bytes)?;
    let all = zip_names(&zip);
    let mut slides: Vec<String> = all.iter().filter(|n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml")).cloned().collect();
    numeric_sort(&mut slides);
    let mut out = String::new();
    for (i, slide) in slides.iter().enumerate() {
        let Some(xml) = zip_entry(&mut zip, slide, 20 << 20) else { continue };
        out.push_str(&format!("\n## 슬라이드 {}\n", i + 1));
        out.push_str(&xml_text(&String::from_utf8_lossy(&xml), &["a:t"], &["a:p", "a:br"], &[]));
        let notes = slide.replace("ppt/slides/slide", "ppt/notesSlides/notesSlide");
        if let Some(xml) = zip_entry(&mut zip, &notes, 20 << 20) {
            let text = xml_text(&String::from_utf8_lossy(&xml), &["a:t"], &["a:p"], &[]);
            let text = text.trim();
            if !text.is_empty() && !text.chars().all(|c| c.is_ascii_digit()) {
                out.push_str("\n[발표자 노트]\n");
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn xlsx(bytes: &[u8]) -> Result<String, String> {
    let mut zip = open_zip(bytes)?;
    let shared: Vec<String> = zip_entry(&mut zip, "xl/sharedStrings.xml", 50 << 20)
        .map(|x| {
            let xml = String::from_utf8_lossy(&x).into_owned();
            xml.split("</si>").filter(|s| s.contains("<si")).map(|si| xml_text(si, &["t"], &[], &[]).trim().to_string()).collect()
        })
        .unwrap_or_default();
    let mut sheets: Vec<String> = zip_names(&zip).into_iter().filter(|n| n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")).collect();
    numeric_sort(&mut sheets);
    let mut out = String::new();
    for (i, sheet) in sheets.iter().enumerate() {
        let Some(xml) = zip_entry(&mut zip, sheet, 50 << 20) else { continue };
        let xml = String::from_utf8_lossy(&xml);
        out.push_str(&format!("\n## 시트 {}\n", i + 1));
        for row in xml.split("</row>") {
            let mut cells = Vec::new();
            for cell in row.split("<c ").skip(1) {
                let head = &cell[..cell.find('>').unwrap_or(0)];
                let shared_ref = head.contains("t=\"s\"");
                let inline = head.contains("t=\"inlineStr\"");
                let value = if inline {
                    xml_text(cell, &["t"], &[], &[])
                } else {
                    cell.split_once("<v>").and_then(|(_, r)| r.split_once("</v>")).map(|(v, _)| decode_entities(v)).unwrap_or_default()
                };
                let value = if shared_ref { value.trim().parse::<usize>().ok().and_then(|i| shared.get(i).cloned()).unwrap_or_default() } else { value };
                cells.push(value);
            }
            while cells.last().is_some_and(|c| c.is_empty()) {
                cells.pop();
            }
            if !cells.is_empty() {
                out.push_str(&cells.join("\t"));
                out.push('\n');
            }
        }
    }
    Ok(out)
}

fn hwpx(bytes: &[u8]) -> Result<String, String> {
    let mut zip = open_zip(bytes)?;
    let mut sections: Vec<String> = zip_names(&zip).into_iter().filter(|n| n.starts_with("Contents/section") && n.ends_with(".xml")).collect();
    numeric_sort(&mut sections);
    let mut out = String::new();
    for s in &sections {
        if let Some(xml) = zip_entry(&mut zip, s, 50 << 20) {
            out.push_str(&xml_text(&String::from_utf8_lossy(&xml), &["hp:t"], &["hp:p", "hp:lineBreak"], &["hp:tab"]));
            out.push('\n');
        }
    }
    Ok(out)
}

/// HWP 5.x: OLE compound file whose BodyText/SectionN streams hold (usually deflated) records;
/// paragraph text is record tag 67 (HWPTAG_PARA_TEXT) in UTF-16LE.
fn hwp(bytes: &[u8]) -> Result<String, String> {
    let mut cfb = cfb::CompoundFile::open(Cursor::new(bytes)).map_err(|_| "한글(.hwp) 파일 구조를 읽지 못했습니다 (HWP 3.0 이하 형식은 지원하지 않음).".to_string())?;
    let mut header = Vec::new();
    cfb.open_stream("/FileHeader").map_err(|_| "한글 파일 헤더가 없습니다.".to_string())?.read_to_end(&mut header).map_err(|e| e.to_string())?;
    let flags = header.get(36..40).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).unwrap_or(0);
    let compressed = flags & 1 != 0;
    if flags & 2 != 0 {
        return Err("암호가 걸린 한글 파일이라 읽을 수 없습니다.".into());
    }
    let distribution = flags & 4 != 0;
    let mut sections: Vec<String> = Vec::new();
    let root = if distribution { "/ViewText" } else { "/BodyText" };
    if let Ok(entries) = cfb.read_storage(root) {
        for e in entries {
            if e.is_stream() && e.name().starts_with("Section") {
                sections.push(format!("{root}/{}", e.name()));
            }
        }
    }
    numeric_sort(&mut sections);
    let mut out = String::new();
    if !distribution {
        for path in &sections {
            let mut raw = Vec::new();
            if cfb.open_stream(path).and_then(|mut s| s.read_to_end(&mut raw)).is_err() {
                continue;
            }
            let data = if compressed {
                let mut inflated = Vec::new();
                if flate2::read::DeflateDecoder::new(&raw[..]).take(200 << 20).read_to_end(&mut inflated).is_err() {
                    continue;
                }
                inflated
            } else {
                raw
            };
            hwp_records(&data, &mut out);
        }
    }
    if out.trim().is_empty() {
        // Fallback: the preview text stream (first pages only).
        let mut prv = Vec::new();
        if cfb.open_stream("/PrvText").and_then(|mut s| s.read_to_end(&mut prv)).is_ok() {
            let (text, _, _) = encoding_rs::UTF_16LE.decode(&prv);
            out = format!("[미리보기 텍스트만 읽었습니다]\n{text}");
        }
    }
    Ok(out)
}

fn hwp_records(data: &[u8], out: &mut String) {
    let mut i = 0;
    while i + 4 <= data.len() {
        let header = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        i += 4;
        let tag = header & 0x3FF;
        let mut size = (header >> 20) as usize;
        if size == 0xFFF {
            if i + 4 > data.len() {
                break;
            }
            size = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
            i += 4;
        }
        if i + size > data.len() {
            break;
        }
        if tag == 67 {
            let units: Vec<u16> = data[i..i + size].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let mut j = 0;
            while j < units.len() {
                let u = units[j];
                match u {
                    // Single-unit control characters.
                    0 | 24..=31 => j += 1,
                    10 => { out.push('\n'); j += 1; }
                    13 => { out.push('\n'); j += 1; }
                    9 => { out.push('\t'); j += 8; }
                    // Inline/extended controls occupy 8 units.
                    1..=8 | 11 | 12 | 14..=23 => j += 8,
                    _ => {
                        if let Some(c) = char::decode_utf16([u]).next().and_then(Result::ok) {
                            out.push(c);
                        } else if j + 1 < units.len() {
                            if let Some(Ok(c)) = char::decode_utf16([u, units[j + 1]]).next() {
                                out.push(c);
                                j += 1;
                            }
                        }
                        j += 1;
                    }
                }
            }
        }
        i += size;
    }
}

fn notebook(bytes: &[u8]) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("노트북(.ipynb)을 읽지 못했습니다: {e}"))?;
    let join = |v: &serde_json::Value| match v {
        serde_json::Value::Array(a) => a.iter().filter_map(|s| s.as_str()).collect::<String>(),
        serde_json::Value::String(s) => s.clone(),
        _ => String::new(),
    };
    let mut out = String::new();
    for (i, cell) in v.get("cells").and_then(|c| c.as_array()).into_iter().flatten().enumerate() {
        let kind = cell.get("cell_type").and_then(|t| t.as_str()).unwrap_or("code");
        out.push_str(&format!("\n# [셀 {} · {kind}]\n", i + 1));
        out.push_str(&join(cell.get("source").unwrap_or(&serde_json::Value::Null)));
        out.push('\n');
        for o in cell.get("outputs").and_then(|o| o.as_array()).into_iter().flatten() {
            let text = o.get("text").map(join).or_else(|| o.pointer("/data/text~1plain").map(join)).unwrap_or_default();
            if !text.trim().is_empty() {
                out.push_str("# [출력]\n");
                out.push_str(&text.chars().take(2000).collect::<String>());
                out.push('\n');
            }
        }
    }
    Ok(out)
}

/// ZIP archives (e.g. submitted projects): list entries and include readable files.
fn archive(bytes: &[u8]) -> Result<String, String> {
    let mut zip = open_zip(bytes)?;
    let names = zip_names(&zip);
    let mut out = format!("[압축 파일 · {}개 항목]\n", names.len());
    for n in names.iter().take(300) {
        out.push_str("- ");
        out.push_str(n);
        out.push('\n');
    }
    let mut budget = 400_000usize;
    for n in &names {
        if n.ends_with('/') || n.contains("__MACOSX") || n.split('/').any(|p| matches!(p, ".git" | "node_modules" | "venv" | ".venv" | "__pycache__" | "build" | "dist" | "bin" | "obj" | ".idea" | ".vs")) {
            continue;
        }
        let ext = extension(n);
        if BINARY_EXTS.contains(&ext.as_str()) || ext == "zip" {
            continue;
        }
        let Some(data) = zip_entry(&mut zip, n, 20 << 20) else { continue };
        let text = if TEXT_EXTS.contains(&ext.as_str()) || ext.is_empty() { decode_text(&data) } else { extract_text(n, &data).ok() };
        let Some(text) = text else { continue };
        if text.trim().is_empty() {
            continue;
        }
        let text: String = text.chars().take(budget).collect();
        budget = budget.saturating_sub(text.chars().count());
        out.push_str(&format!("\n===== {n} =====\n{text}\n"));
        if budget == 0 {
            out.push_str("\n…(압축 파일이 커서 나머지 파일은 생략)\n");
            break;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (n, c) in files {
                z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(c.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn reads_code_in_utf8_and_cp949() {
        assert_eq!(extract_text("main.py", "print('안녕')\n".as_bytes()).unwrap(), "print('안녕')");
        let (cp949, _, _) = encoding_rs::EUC_KR.encode("// 과제 1\nint main(){}");
        assert_eq!(extract_text("a.c", &cp949).unwrap(), "// 과제 1\nint main(){}");
        assert!(extract_text("photo.png", b"\x89PNG....").is_err());
    }

    #[test]
    fn reads_docx_pptx_xlsx_hwpx() {
        let docx = make_zip(&[("word/document.xml", "<w:document><w:body><w:p><w:r><w:t>1주차 &amp; 소개</w:t></w:r></w:p><w:p><w:r><w:t>둘째 줄</w:t></w:r></w:p></w:body></w:document>")]);
        assert_eq!(extract_text("a.docx", &docx).unwrap(), "1주차 & 소개\n둘째 줄");
        let pptx = make_zip(&[
            ("ppt/slides/slide10.xml", "<p:sld><a:p><a:r><a:t>열번째</a:t></a:r></a:p></p:sld>"),
            ("ppt/slides/slide2.xml", "<p:sld><a:p><a:r><a:t>두번째</a:t></a:r></a:p></p:sld>"),
            ("ppt/notesSlides/notesSlide2.xml", "<p:notes><a:p><a:t>노트</a:t></a:p></p:notes>"),
        ]);
        let t = extract_text("l.pptx", &pptx).unwrap();
        assert!(t.find("두번째").unwrap() < t.find("열번째").unwrap(), "{t}");
        assert!(t.contains("[발표자 노트]\n노트"));
        let xlsx = make_zip(&[
            ("xl/sharedStrings.xml", "<sst><si><t>이름</t></si><si><t>점수</t></si></sst>"),
            ("xl/worksheets/sheet1.xml", r#"<worksheet><sheetData><row><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row><row><c r="A2" t="inlineStr"><is><t>홍길동</t></is></c><c r="B2"><v>95</v></c></row></sheetData></worksheet>"#),
        ]);
        assert_eq!(extract_text("s.xlsx", &xlsx).unwrap(), "## 시트 1\n이름\t점수\n홍길동\t95");
        let hwpx = make_zip(&[("Contents/section0.xml", "<hs:sec><hp:p><hp:run><hp:t>한글 문서</hp:t></hp:run></hp:p></hs:sec>")]);
        assert_eq!(extract_text("r.hwpx", &hwpx).unwrap(), "한글 문서");
    }

    #[test]
    fn reads_zip_projects_and_notebooks() {
        let z = make_zip(&[("proj/main.cpp", "int main() { return 0; }"), ("proj/build/out.o", "xx"), ("proj/README.md", "# 과제")]);
        let t = extract_text("submission.zip", &z).unwrap();
        assert!(t.contains("===== proj/main.cpp =====\nint main() { return 0; }"));
        assert!(t.contains("# 과제"));
        assert!(!t.contains("===== proj/build/out.o"));
        let nb = r##"{"cells":[{"cell_type":"code","source":["x = 1\n","print(x)"],"outputs":[{"text":["1\n"]}]}]}"##;
        let t = extract_text("hw.ipynb", nb.as_bytes()).unwrap();
        assert!(t.contains("print(x)") && t.contains("# [출력]\n1"));
    }

    #[test]
    fn parses_hwp_para_text_records() {
        let text: Vec<u16> = "가나".encode_utf16().chain([13u16]).collect();
        let mut rec = Vec::new();
        let size = (text.len() * 2) as u32;
        rec.extend_from_slice(&(67u32 | (size << 20)).to_le_bytes());
        for u in text {
            rec.extend_from_slice(&u.to_le_bytes());
        }
        let mut out = String::new();
        hwp_records(&rec, &mut out);
        assert_eq!(out, "가나\n");
    }

    #[test]
    fn pages_long_text() {
        let (chunk, next, total) = page("가나다라마", 1, 2);
        assert_eq!((chunk.as_str(), next, total), ("나다", Some(3), 5));
        assert_eq!(page("가나", 0, 5).1, None);
    }
}

#[cfg(test)]
mod manual {
    /// `EXTRACT_FILES=a.pdf:b.pptx cargo test extract_env_files -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn extract_env_files() {
        for path in std::env::var("EXTRACT_FILES").unwrap_or_default().split(':').filter(|p| !p.is_empty()) {
            let bytes = std::fs::read(path).unwrap();
            println!("=== {path}\n{:?}", super::extract_text(path, &bytes));
        }
    }
}
