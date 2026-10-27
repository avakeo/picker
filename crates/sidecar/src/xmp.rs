use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmpError {
    UnsafeXml,
    Malformed(&'static str),
}

impl std::fmt::Display for XmpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsafeXml => formatter.write_str("DTD and entity declarations are not allowed"),
            Self::Malformed(message) => write!(formatter, "malformed XMP: {message}"),
        }
    }
}

impl std::error::Error for XmpError {}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn validate(xml: &str) -> Result<(), XmpError> {
    let upper = xml.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err(XmpError::UnsafeXml);
    }
    Ok(())
}

fn existing_items(block: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut rest = block;
    while let Some(start) = rest.find("<rdf:li") {
        let Some(open_end) = rest[start..].find('>') else {
            break;
        };
        let content_start = start + open_end + 1;
        let Some(close) = rest[content_start..].find("</rdf:li>") else {
            break;
        };
        items.push(rest[content_start..content_start + close].trim().to_owned());
        rest = &rest[content_start + close + "</rdf:li>".len()..];
    }
    items
}

fn block_bounds(xml: &str, prefix: &str) -> Option<(usize, usize)> {
    let start = xml.find(&format!("<{prefix}"))?;
    let tag_end = xml[start..].find('>')? + start;
    let close = format!("</{prefix}>");
    let end = xml[tag_end..].find(&close)? + tag_end + close.len();
    Some((start, end))
}

fn append_items(block: &str, tag: &str, values: &[String]) -> Result<String, XmpError> {
    let mut existing = existing_items(block);
    existing.sort();
    existing.dedup();
    let mut additions = Vec::new();
    for value in values {
        let escaped = escape(value);
        if !existing.contains(&escaped) {
            additions.push(escaped);
        }
    }
    if additions.is_empty() {
        return Ok(block.to_owned());
    }
    let close = "</rdf:Bag>";
    let Some(position) = block.rfind(close) else {
        return Err(XmpError::Malformed("missing rdf:Bag"));
    };
    let mut output = String::with_capacity(block.len() + additions.len() * 32);
    output.push_str(&block[..position]);
    for value in additions {
        write!(output, "<rdf:li>{value}</rdf:li>").expect("write to string");
    }
    output.push_str(&block[position..]);
    let _ = tag;
    Ok(output)
}

/// Merges keywords into an XMP packet, preserving unrelated fields and items.
pub fn merge(
    existing: Option<&str>,
    keywords: &[String],
    hierarchical: &[String],
) -> Result<String, XmpError> {
    let mut xml = existing.unwrap_or("").to_owned();
    validate(&xml)?;
    if xml.trim().is_empty() {
        xml = "<?xpacket begin=\"﻿\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"></rdf:Description>\
</rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
            .to_owned();
    }
    if !xml.contains("xmlns:dc=") {
        xml = xml.replacen(
            "<rdf:Description",
            "<rdf:Description xmlns:dc=\"http://purl.org/dc/elements/1.1/\"",
            1,
        );
    }
    if !xml.contains("xmlns:lr=") {
        xml = xml.replacen(
            "<rdf:Description",
            "<rdf:Description xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\"",
            1,
        );
    }
    if let Some((start, end)) = block_bounds(&xml, "dc:subject") {
        let block = append_items(&xml[start..end], "dc:subject", keywords)?;
        xml.replace_range(start..end, &block);
    } else if !keywords.is_empty() {
        let block = format!(
            "<dc:subject><rdf:Bag>{}</rdf:Bag></dc:subject>",
            keywords
                .iter()
                .map(|value| format!("<rdf:li>{}</rdf:li>", escape(value)))
                .collect::<String>()
        );
        xml = insert_block(xml, &block)?;
    }
    if let Some((start, end)) = block_bounds(&xml, "lr:hierarchicalSubject") {
        let block = append_items(&xml[start..end], "lr:hierarchicalSubject", hierarchical)?;
        xml.replace_range(start..end, &block);
    } else if !hierarchical.is_empty() {
        let block = format!(
            "<lr:hierarchicalSubject><rdf:Bag>{}</rdf:Bag></lr:hierarchicalSubject>",
            hierarchical
                .iter()
                .map(|value| format!("<rdf:li>{}</rdf:li>", escape(value)))
                .collect::<String>()
        );
        xml = insert_block(xml, &block)?;
    }
    Ok(xml)
}

fn insert_block(mut xml: String, block: &str) -> Result<String, XmpError> {
    if let Some(position) = xml.rfind("</rdf:Description>") {
        xml.insert_str(position, block);
        return Ok(xml);
    }
    if let Some(position) = xml.rfind("</rdf:RDF>") {
        xml.insert_str(
            position,
            &format!(
                "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:lr=\"http://ns.adobe.com/lightroom/1.0/\">{block}</rdf:Description>"
            ),
        );
        return Ok(xml);
    }
    Err(XmpError::Malformed("missing rdf:Description"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_unrelated_fields_and_merges_duplicates() {
        let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><xmp:Rating>5</xmp:Rating><dc:subject><rdf:Bag><rdf:li>海辺</rdf:li><rdf:li>other</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let merged = merge(
            Some(existing),
            &["海辺".to_owned(), "夕日".to_owned()],
            &["景色|海辺".to_owned()],
        )
        .expect("merge");
        assert!(merged.contains("<xmp:Rating>5</xmp:Rating>"));
        assert_eq!(merged.matches("<rdf:li>海辺</rdf:li>").count(), 1);
        assert!(merged.contains("<rdf:li>夕日</rdf:li>"));
        assert!(merged.contains("<rdf:li>other</rdf:li>"));
        assert!(merged.contains("lr:hierarchicalSubject"));
    }

    #[test]
    fn creates_a_packet_and_escapes_values() {
        let xml = merge(None, &["a<b&c".to_owned()], &[]).expect("merge");
        assert!(xml.contains("a&lt;b&amp;c"));
        assert!(xml.contains("dc:subject"));
    }

    #[test]
    fn refuses_dtd_and_entities() {
        assert_eq!(
            merge(
                Some("<!DOCTYPE x [<!ENTITY e SYSTEM 'file:///etc/passwd'>]>"),
                &[],
                &[]
            ),
            Err(XmpError::UnsafeXml)
        );
        assert_eq!(
            merge(Some("<!ENTITY xxe SYSTEM 'file:///etc/passwd'>"), &[], &[]),
            Err(XmpError::UnsafeXml)
        );
    }
}
