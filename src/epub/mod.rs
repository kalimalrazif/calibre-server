use crate::db::models::BookMetadata;
use std::io::{Read, Write};
use zip::write::FileOptions;
use zip::{ZipArchive, ZipWriter};

/// Update EPUB metadata with current database information
pub fn update_epub_metadata(
    epub_path: &std::path::Path,
    metadata: &BookMetadata,
) -> Result<Vec<u8>, String> {
    // Read the original EPUB file
    let file = std::fs::File::open(epub_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| e.to_string())?;

    // Create a new EPUB in memory
    let mut output = Vec::new();
    {
        let mut zip_writer = ZipWriter::new(std::io::Cursor::new(&mut output));

        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);

        // Copy all files from original EPUB
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = file.name().to_string();

            // If it's the OPF file (metadata), update it
            if name.ends_with(".opf") || name.contains("content.opf") {
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .map_err(|e| e.to_string())?;

                // Update metadata in OPF
                let updated_content = update_opf_metadata(&content, metadata)?;

                zip_writer
                    .start_file(&name, options)
                    .map_err(|e| e.to_string())?;
                zip_writer
                    .write_all(updated_content.as_bytes())
                    .map_err(|e| e.to_string())?;
            } else {
                // Copy file as-is
                zip_writer
                    .start_file(&name, options)
                    .map_err(|e| e.to_string())?;
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;
                zip_writer.write_all(&buffer).map_err(|e| e.to_string())?;
            }
        }

        zip_writer.finish().map_err(|e| e.to_string())?;
    }

    Ok(output)
}

/// Update metadata in OPF XML content
fn update_opf_metadata(opf_content: &str, metadata: &BookMetadata) -> Result<String, String> {
    // For simplicity, we'll do a basic XML manipulation
    // In a production system, you'd want proper XML parsing

    let mut updated = opf_content.to_string();

    // Update title
    if let Some(start) = updated.find("<dc:title>")
        && let Some(end) = updated[start..].find("</dc:title>")
    {
        let end_pos = start + end;
        let new_title = format!("<dc:title>{}</dc:title>", escape_xml(&metadata.book.title));
        updated.replace_range(start..end_pos + 11, &new_title);
    }

    // Update authors
    if !metadata.authors.is_empty() {
        let author_name = &metadata.authors[0].name;
        if let Some(start) = updated.find("<dc:creator")
            && let Some(end) = updated[start..].find("</dc:creator>")
        {
            let tag_end = updated[start..].find('>').unwrap() + start + 1;
            let close_start = start + end;
            let new_author = escape_xml(author_name);
            updated.replace_range(tag_end..close_start, &new_author);
        }
    }

    Ok(updated)
}

/// Escape XML special characters
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
