# User guide

Notercrux is pre-alpha. This guide covers only what works today.

## Pages

- **New page:** `⌘/Ctrl+N`, the **+** in the sidebar, or the command palette.
- **Nest:** hover a page and use **+**, or right-click → _Add nested page_.
- **Rename:** edit the title at the top of the page, or `F2` / right-click → _Rename_ in the sidebar.
- **Duplicate:** right-click → _Duplicate_ (copies nested pages and content).
- **Archive / restore / delete:** _Move to archive_ (or `Delete` in the sidebar) hides a page and
  everything inside it. Open **Archive** at the bottom of the sidebar to restore or delete forever.
  Deleting forever asks for confirmation and cannot be undone.

## Editing

- **Slash menu:** type `/` on any line to insert a block — text, headings, lists, to-do, quote,
  callout, code, divider. Keep typing to filter (`/todo`, `/h2`), arrows and `Enter` to choose.
- **Markdown shortcuts:** `# `, `## ` headings, `- ` or `1. ` lists, `[ ] ` to-do, `> ` quote,
  three backticks for code, `**bold**`, `*italic*`, `` `code` ``. `Tab` / `Shift+Tab` nest list items.
- **Formatting:** select text to get a floating toolbar (bold, italic, strikethrough, code, link).
- **Moving blocks:** drag the `⋮⋮` handle that appears beside a block, or use `Alt+↑/↓`
  (`⌘/Ctrl+Shift+↑/↓`) to move the block under the cursor. List items move with their children.
- **Undo/redo:** `⌘/Ctrl+Z`, `⌘/Ctrl+Shift+Z`. History is kept while a page is open and is
  discarded when you switch pages.
- Changes save automatically a moment after you stop typing; the corner shows _Saving…_ /
  _Saved_, or an error if something could not be saved.

## Organizing pages

Drag a page in the sidebar: the top edge of a row puts it before, the bottom edge after, and the
middle nests it inside. From the keyboard, focus a page and use `Alt+↑/↓` to reorder and
`Alt+→/←` to indent under the previous page or outdent. A page cannot be moved into its own
subpages.

## Export and import

Open the command palette (`⌘/Ctrl+K`):

- **Export page as Markdown…** writes a `.md` file. Callouts become block quotes and underline is
  dropped; Notercrux lists every such loss after exporting.
- **Export page as JSON (lossless)…** keeps everything and can be re-imported exactly.
- **Import Markdown or JSON file…** creates a new page. A leading `# Heading` becomes the title.
  Tables, images, HTML and headings deeper than level 3 are not supported: they are imported as
  plain text or links, and the notes shown after import say what was affected.

## Appearance

Settings (`⌘/Ctrl+,`): theme, glass effects (reduced transparency uses solid surfaces) and motion.
Each defaults to following your system.

## Not available yet

Tables, images and file attachments, page links, bulk export/import, databases, sync, encryption.
