use crate::ids::PageId;
use crate::page::Page;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TreeError {
    #[error("page {0} not found")]
    NotFound(PageId),
    #[error("cannot move a page into itself or one of its descendants")]
    Cycle,
}

/// In-memory view of the page hierarchy, used to validate structural operations
/// before they are persisted.
#[derive(Default)]
pub struct PageTree {
    parents: HashMap<PageId, Option<PageId>>,
}

impl PageTree {
    pub fn from_pages<'a>(pages: impl IntoIterator<Item = &'a Page>) -> Self {
        Self {
            parents: pages.into_iter().map(|p| (p.id, p.parent_id)).collect(),
        }
    }

    /// Ids of `id` and all its descendants.
    pub fn subtree(&self, id: PageId) -> Vec<PageId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            let cur = out[i];
            out.extend(
                self.parents
                    .iter()
                    .filter(|(_, p)| **p == Some(cur))
                    .map(|(c, _)| *c),
            );
            i += 1;
        }
        out
    }

    /// Checks that moving `id` under `new_parent` keeps the hierarchy a forest.
    pub fn check_move(&self, id: PageId, new_parent: Option<PageId>) -> Result<(), TreeError> {
        if !self.parents.contains_key(&id) {
            return Err(TreeError::NotFound(id));
        }
        let mut cursor = new_parent;
        while let Some(p) = cursor {
            if p == id {
                return Err(TreeError::Cycle);
            }
            cursor = *self.parents.get(&p).ok_or(TreeError::NotFound(p))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(id: PageId, parent: Option<PageId>) -> Page {
        Page {
            id,
            parent_id: parent,
            title: String::new(),
            icon: None,
            position: 0,
            created_at: 0,
            updated_at: 0,
            archived_at: None,
        }
    }

    #[test]
    fn rejects_cycles() {
        let (a, b, c) = (PageId::new(), PageId::new(), PageId::new());
        let pages = [page(a, None), page(b, Some(a)), page(c, Some(b))];
        let t = PageTree::from_pages(&pages);
        assert_eq!(t.check_move(a, Some(c)), Err(TreeError::Cycle));
        assert_eq!(t.check_move(a, Some(a)), Err(TreeError::Cycle));
        assert!(t.check_move(c, Some(a)).is_ok());
        assert!(t.check_move(c, None).is_ok());
        assert_eq!(t.subtree(a).len(), 3);
    }
}
