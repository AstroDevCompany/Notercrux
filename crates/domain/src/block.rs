use crate::ids::BlockId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Bump when the persisted document shape changes; readers must refuse newer versions.
pub const DOCUMENT_SCHEMA_VERSION: u32 = 1;

/// Block types supported by the initial editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlockKind {
    Paragraph,
    Heading,
    BulletedListItem,
    NumberedListItem,
    Todo,
    Quote,
    Callout,
    Code,
    Divider,
    PageLink,
}

/// Inline formatting over a half-open character range `[from, to)` of a block's text.
/// Offsets count Unicode scalar values (not bytes or UTF-16 units) so every platform agrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    pub from: u32,
    pub to: u32,
    /// `bold`, `italic`, `strike`, `code`, `link`, ...
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, Value>,
}

/// A block with a stable id, an explicit type, typed attributes and nested children.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: BlockId,
    #[serde(rename = "type")]
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attrs: Map<String, Value>,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Block>,
}

impl Block {
    pub fn new(kind: BlockKind, text: impl Into<String>) -> Self {
        Self {
            id: BlockId::new(),
            kind,
            attrs: Map::new(),
            text: text.into(),
            marks: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        let mut b = Self::new(BlockKind::Heading, text);
        b.attrs
            .insert("level".into(), Value::from(level.clamp(1, 3)));
        b
    }

    fn visit<'a>(&'a self, f: &mut impl FnMut(&'a Block)) {
        f(self);
        for c in &self.children {
            c.visit(f);
        }
    }
}

/// The content of one page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub version: u32,
    pub blocks: Vec<Block>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: DOCUMENT_SCHEMA_VERSION,
            blocks: Vec::new(),
        }
    }
}

impl Document {
    /// Plain text of every block, one block per line. Feeds the local search index.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        for b in &self.blocks {
            b.visit(&mut |blk| {
                if !blk.text.is_empty() {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(&blk.text);
                }
            });
        }
        out
    }

    pub fn block_count(&self) -> usize {
        let mut n = 0;
        for b in &self.blocks {
            b.visit(&mut |_| n += 1);
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_preserves_ids_and_structure() {
        let mut parent = Block::new(BlockKind::BulletedListItem, "parent");
        parent.children.push(Block::new(BlockKind::Todo, "child"));
        let doc = Document {
            version: DOCUMENT_SCHEMA_VERSION,
            blocks: vec![Block::heading(1, "Title"), parent],
        };
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert_eq!(back.block_count(), 3);
        assert_eq!(back.plain_text(), "Title\nparent\nchild");
    }

    #[test]
    fn unknown_block_kind_is_rejected_not_dropped() {
        let json = r#"{"version":1,"blocks":[{"id":"0190a000-0000-7000-8000-000000000000","type":"hologram","text":""}]}"#;
        assert!(serde_json::from_str::<Document>(json).is_err());
    }
}
