use crate::db::models::BookMetadata;
use quick_xml::events::{BytesText, Event};
use quick_xml::{Reader, Writer};
use std::io::{Cursor, Read, Write};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

/// Update EPUB metadata with current database information.
///
/// Rebuilds the EPUB in memory, rewriting the OPF package document's title
/// and first creator from `metadata`. The OCF `mimetype` entry is re-emitted
/// first and *uncompressed* (STORED), as the EPUB spec requires — otherwise
/// strict readers reject the file.
pub fn update_epub_metadata(
    epub_path: &std::path::Path,
    metadata: &BookMetadata,
) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(epub_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;

    let mut output = Vec::new();
    {
        let mut zip_writer = ZipWriter::new(Cursor::new(&mut output));

        let stored: FileOptions =
            FileOptions::default().compression_method(CompressionMethod::Stored);
        let deflated: FileOptions = FileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // EPUB/OCF rule: the "mimetype" entry must be the FIRST file and
        // STORED (uncompressed). Write it ourselves first, then skip it in
        // the copy loop below.
        zip_writer
            .start_file("mimetype", stored)
            .map_err(|e| e.to_string())?;
        zip_writer
            .write_all(b"application/epub+zip")
            .map_err(|e| e.to_string())?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();

            if name == "mimetype" {
                continue; // already written, STORED-first
            }

            if name.ends_with(".opf") || name.contains("content.opf") {
                let mut content = String::new();
                entry
                    .read_to_string(&mut content)
                    .map_err(|e| e.to_string())?;
                let updated = update_opf_metadata(&content, metadata)?;
                zip_writer
                    .start_file(&name, deflated)
                    .map_err(|e| e.to_string())?;
                zip_writer
                    .write_all(updated.as_bytes())
                    .map_err(|e| e.to_string())?;
            } else {
                zip_writer
                    .start_file(&name, deflated)
                    .map_err(|e| e.to_string())?;
                let mut buffer = Vec::new();
                entry.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
                zip_writer.write_all(&buffer).map_err(|e| e.to_string())?;
            }
        }

        zip_writer.finish().map_err(|e| e.to_string())?;
    }

    Ok(output)
}

/// Rewrite `<dc:title>` and the first `<dc:creator>` text in an OPF package
/// document, using a proper XML parser so namespaces, attributes and
/// self-closing tags don't corrupt the file.
///
/// ponytail: only title + first author are updated, matching the previous
/// behaviour. Other metadata is passed through untouched.
fn update_opf_metadata(opf_content: &str, metadata: &BookMetadata) -> Result<String, String> {
    let mut reader = Reader::from_str(opf_content);
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    // Which element's text we're currently inside and should replace.
    #[derive(PartialEq)]
    enum Replacing {
        None,
        Title,
        Creator,
    }
    let mut replacing = Replacing::None;
    let mut creator_done = false;
    let new_author = metadata.authors.first().map(|a| a.name.clone());

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("OPF parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                replacing = match local.as_str() {
                    "title" => Replacing::Title,
                    "creator" if !creator_done && new_author.is_some() => Replacing::Creator,
                    _ => Replacing::None,
                };
                writer
                    .write_event(Event::Start(e.clone()))
                    .map_err(|e| e.to_string())?;
            }
            Ok(Event::End(e)) => {
                if replacing == Replacing::Creator {
                    creator_done = true;
                }
                replacing = Replacing::None;
                writer
                    .write_event(Event::End(e.clone()))
                    .map_err(|e| e.to_string())?;
            }
            Ok(Event::Text(t)) => match replacing {
                Replacing::Title => {
                    writer
                        .write_event(Event::Text(BytesText::new(&metadata.book.title)))
                        .map_err(|e| e.to_string())?;
                }
                Replacing::Creator => {
                    let name = new_author.as_deref().unwrap_or_default();
                    writer
                        .write_event(Event::Text(BytesText::new(name)))
                        .map_err(|e| e.to_string())?;
                }
                Replacing::None => {
                    writer
                        .write_event(Event::Text(t))
                        .map_err(|e| e.to_string())?;
                }
            },
            Ok(ev) => {
                writer.write_event(ev).map_err(|e| e.to_string())?;
            }
        }
    }

    let bytes = writer.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Strip an optional `prefix:` from a (possibly namespaced) XML tag name.
fn local_name(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    match name.rsplit_once(':') {
        Some((_, local)) => local.to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::models::{Author, Book, BookMetadata};
    use std::collections::HashMap;

    fn meta(title: &str, author: &str) -> BookMetadata {
        BookMetadata {
            book: Book {
                id: 1,
                title: title.to_string(),
                sort: None,
                timestamp: None,
                pubdate: None,
                series_index: None,
                author_sort: None,
                path: String::new(),
                uuid: None,
                has_cover: None,
                last_modified: None,
            },
            authors: vec![Author {
                id: 1,
                name: author.to_string(),
                sort: None,
                link: None,
            }],
            formats: vec![],
            custom_columns: HashMap::new(),
            series_id: None,
            series_name: None,
            series_index: None,
            tags: vec![],
            publisher: None,
            comment: None,
        }
    }

    #[test]
    fn rewrites_title_and_first_creator_preserving_attrs() {
        // Namespaced tags + attributes + a second creator that must be left alone.
        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title id="t1">Old Title</dc:title>
    <dc:creator opf:role="aut">Old Author</dc:creator>
    <dc:creator opf:role="ill">Illustrator</dc:creator>
  </metadata>
</package>"#;

        let out = update_opf_metadata(opf, &meta("New & Shiny", "Jane <Doe>")).unwrap();

        // Title replaced, with XML special chars escaped by the writer.
        assert!(out.contains("<dc:title id=\"t1\">New &amp; Shiny</dc:title>"));
        // First creator replaced (and escaped), attribute preserved.
        assert!(out.contains("opf:role=\"aut\">Jane &lt;Doe&gt;</dc:creator>"));
        // Second creator untouched.
        assert!(out.contains("opf:role=\"ill\">Illustrator</dc:creator>"));
    }
}
