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

## Block options and right-click

- **Block handle (`⋮⋮`):** hover a block and **click** the handle. The block is selected and a
  menu opens with the block's properties, **Turn into**, Duplicate, Move up/down, Copy and Delete.
  **Drag** the handle to move the block instead.
- **Keyboard:** `Esc` selects the block under the cursor (`Esc` again goes back to the text), `Enter`
  opens its menu, `Delete` removes it, `⌥↑/↓` moves it and `⌘/Ctrl+D` duplicates it.
- **Right-click** in a page for Cut, Copy, Paste (and paste as plain text), formatting for the
  selected text, **Turn into**, and block actions. Over a page link it also offers _Open linked page_
  and _Remove link_. Right-click a database row for Open, Duplicate and Archive; elsewhere you get a
  short app menu (new page, back/forward, search, settings). Text fields keep the system menu.
- **Code blocks** have a language selector (and a Copy button) in their header, and are
  syntax-highlighted. **Callouts** have a clickable icon that opens an icon and colour picker; any
  emoji can be pasted in.

## Selecting several blocks

Press in the empty margin beside the text (or in the empty space below it) and drag: a band appears
and every block it touches is selected. Dragging near the top or bottom edge scrolls the page. A
lone list, quote or callout counts as one block. With blocks selected:

- `Delete` / `Backspace` removes them, `⌘/Ctrl+C` and `⌘/Ctrl+X` copy or cut whole blocks (pasting
  elsewhere keeps their types), `⌘/Ctrl+D` duplicates them.
- Right-click for Copy, Cut, Duplicate, **Turn into** (applies to every block) and Delete.
- `Esc` (or an arrow key) leaves the selection; clicking in the margin does too. Typing does not
  replace the blocks: it leaves the selection and types at its end. `Alt+↑/↓` is ignored while
  several blocks are selected (moving a range is not supported yet).

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

## Navigation and search

- **Command palette** (`⌘/Ctrl+K`): with nothing typed it lists commands and your recent pages.
  Type to search page titles _and_ page content; results from content show a highlighted excerpt.
  Choose **Create page “…”** to make a page from what you typed.
- **Back / forward:** the arrows in the top bar, or `⌘/Ctrl+[` and `⌘/Ctrl+]`. Pages that were
  archived or deleted are skipped.
- **Breadcrumbs** at the top show where a page lives; click a parent to open it.
- **Sidebar keyboard:** arrows move, `→`/`←` expand/collapse, `Enter` opens, `F2` renames,
  `Delete` archives, `Alt+arrows` move pages, the context-menu key opens actions. `⌘/Ctrl+\`
  toggles the sidebar.

## Page links

Type `[[` in a page and pick a page (or create a new one) to insert a link. Click a link to open
the page. Links follow the page even if it is renamed or moved, but the visible text keeps the title
it had when you inserted it. Every page lists the pages that link to it under **Linked from**.

## Databases

Create one with the table button next to **Pages** in the sidebar (or **New database** in the
palette). A database is a table of records; each record is a page you can open and write in.

- **Properties** (columns): text, number, select, multi-select, date, checkbox, URL and relation
  (links to records of another database). Add, rename, retype, hide or delete them under
  **Properties**; select options are edited there too. Changing a property's type or deleting it
  clears its values, and you are asked first.
- **Filter** and **Sort** apply to the current **view**. Filters can match all or any condition;
  select columns sort in the order of their options; empty values always sort last. Use **+** next
  to the view tabs for more views (double-click a tab to rename it, right-click to delete).
- Edit cells in place. **Open** (on row hover) opens the record as a page, with its properties above
  the content. Records cannot be moved to another database.

## Appearance and settings

Settings (`⌘/Ctrl+,`):

- **Appearance:** theme, accent color, glass effects (reduced transparency uses solid surfaces) and
  motion. Theme, glass and motion default to following your system.
- **Pages:** typeface (sans, serif, mono), text size and page width.
- **Keyboard shortcuts:** click a shortcut, then press the new keys. A shortcut must include
  ⌘/Ctrl or Alt, and one already in use is refused. Editor shortcuts and sidebar keys are fixed.

## Not available yet

Editor tables, images and file attachments, bulk export/import, two-way relations, formulas, sync, encryption.
