import Foundation
import Observation

/// Edits one page. Saves go through the core's rebased save, so changes that arrive from other
/// devices while the person types are merged instead of overwritten (same rule as the desktop app).
@MainActor
@Observable
final class PageEditor {
    let pageId: String
    private let core: Core
    private(set) var doc = Document()
    private var base = Document()
    private var version = 0
    private var dirty = false
    private var timer: Task<Void, Never>?
    var focus: String?
    var saveError: String?
    var conflictNotice: String?
    var loadError: String?

    init(pageId: String, core: Core) {
        self.pageId = pageId
        self.core = core
        load()
    }

    func load() {
        do {
            let d = try Coding.decode(Document.self, try core.loadDocumentJson(id: pageId))
            doc = d; base = d; loadError = nil
        } catch { loadError = "\(error)" }
    }

    // ---- editing ----------------------------------------------------------------------------

    func block(_ id: String) -> Block? { BlockOps.find(doc.blocks, id: id) }

    func setText(_ id: String, _ new: String) {
        guard let current = block(id), current.text != new else { return }
        if new.contains("\n"), !BlockOps.keepsNewlines(current.type) {
            splitAtNewline(id, new)
            return
        }
        change { blocks in
            _ = BlockOps.update(&blocks, id: id) { b in
                b.marks = BlockOps.adjust(marks: b.marks, old: b.text, new: new)
                b.text = new
            }
        }
    }

    private func splitAtNewline(_ id: String, _ new: String) {
        guard var b = block(id) else { return }
        let scalars = Array(new.unicodeScalars)
        let at = scalars.firstIndex(of: "\n") ?? scalars.count
        // First fold the typed text into the block (so marks adjust), then split.
        b.marks = BlockOps.adjust(marks: b.marks, old: b.text, new: new)
        b.text = new
        let (head, tail) = BlockOps.split(b, at: at)
        let list = ["bulletedListItem", "numberedListItem", "todo", "quote"].contains(b.type)
        // Return on an empty list item leaves the list.
        if list, head.text.isEmpty, tail.text.isEmpty {
            change { blocks in _ = BlockOps.update(&blocks, id: id) { BlockOps.turn(&$0, into: "paragraph"); $0.text = "" ; $0.marks = nil } }
            focus = id
            return
        }
        var newBlock = tail
        newBlock.type = BlockOps.continuation(of: b.type)
        if newBlock.type == "todo" { newBlock.attrs = ["checked": .bool(false)] }
        else if newBlock.type == "paragraph" { newBlock.attrs = nil }
        newBlock.children = nil
        let newId = newBlock.id
        change { blocks in
            _ = BlockOps.update(&blocks, id: id) { $0 = head; $0.children = b.children }
            BlockOps.insert(&blocks, newBlock, after: id)
        }
        focus = newId
    }

    /// Backspace pressed at the very start of a block.
    /// With `settle`, focus moves to the previous block first and the change follows a moment later.
    /// Removing the focused field in the same pass drops first responder for an instant, which makes
    /// the keyboard hide and reappear.
    func backspaceAtStart(_ id: String, settle: Bool = true) {
        guard let b = block(id) else { return }
        let rows = BlockOps.rows(doc.blocks)
        let idx = rows.firstIndex { $0.id == id }
        let previous = idx.flatMap { $0 > 0 ? rows[$0 - 1].block : nil }
        if b.text.isEmpty {
            // First leave a list/heading/quote, then remove the block.
            if b.type != "paragraph" {
                turn(id, into: "paragraph")
                focus = id
            } else if (b.children ?? []).isEmpty {
                if let previous { focus = previous.id }
                afterFocusMoves(settle && previous != nil) { self.delete(id) }
            }
            return
        }
        guard let previous, BlockOps.textKinds.contains(previous.type), (previous.children ?? []).isEmpty,
              rows[idx! - 1].depth == rows[idx!].depth else { focus = id; return }
        let merged = BlockOps.merge(head: previous, tail: b)
        focus = previous.id
        afterFocusMoves(settle) {
            self.change { blocks in
                _ = BlockOps.update(&blocks, id: previous.id) { $0 = merged }
                BlockOps.remove(&blocks, id: id)
            }
            self.focus = previous.id
        }
    }

    private func afterFocusMoves(_ wait: Bool, _ work: @escaping @MainActor () -> Void) {
        guard wait else { work(); return }
        Task { @MainActor in
            try? await Task.sleep(for: .milliseconds(80))
            work()
        }
    }

    func applySlash(_ id: String, _ item: BlockOps.SlashItem) {
        change { blocks in _ = BlockOps.update(&blocks, id: id) { b in
            b.text = ""; b.marks = nil
            BlockOps.turn(&b, into: item.kind, level: item.level) } }
        if item.kind == "divider" { addBlock(after: id) } else { focus = id }
    }

    func move(_ id: String, by delta: Int) {
        change { blocks in BlockOps.move(&blocks, id: id, by: delta) }
    }

    func toggleChecked(_ id: String) {
        change { blocks in _ = BlockOps.update(&blocks, id: id) { b in
            var a = b.attrs ?? [:]; a["checked"] = .bool(!b.checked); b.attrs = a } }
    }

    func turn(_ id: String, into kind: String, level: Int? = nil) {
        change { blocks in _ = BlockOps.update(&blocks, id: id) { BlockOps.turn(&$0, into: kind, level: level) } }
    }

    func setCaption(_ id: String, _ text: String) {
        change { blocks in _ = BlockOps.update(&blocks, id: id) { $0.text = text } }
    }

    @discardableResult
    func addBlock(after id: String?, kind: String = "paragraph") -> String {
        var b = Block(type: kind)
        if kind == "todo" { b.attrs = ["checked": .bool(false)] }
        if kind == "heading" { b.attrs = ["level": .int(1)] }
        change { blocks in BlockOps.insert(&blocks, b, after: id) }
        focus = b.id
        return b.id
    }

    func duplicate(_ id: String) {
        guard var copy = block(id) else { return }
        func fresh(_ b: inout Block) { b.id = Block.newId(); for i in (b.children ?? []).indices { fresh(&b.children![i]) } }
        fresh(&copy)
        change { blocks in BlockOps.insert(&blocks, copy, after: id) }
    }

    func delete(_ id: String) {
        change { blocks in BlockOps.remove(&blocks, id: id) }
    }

    private func change(_ f: (inout [Block]) -> Void) {
        var blocks = doc.blocks
        f(&blocks)
        guard blocks != doc.blocks else { return }
        doc.blocks = blocks
        dirty = true
        version += 1
        timer?.cancel()
        timer = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(400))
            if !Task.isCancelled { self?.save() }
        }
    }

    // ---- saving and merging -----------------------------------------------------------------

    func save() {
        timer?.cancel()
        guard dirty else { return }
        let sent = doc, v = version
        do {
            let res = try Coding.decode(Rebased.self,
                try core.saveDocumentRebasedJson(id: pageId, baseJson: Coding.encode(base), docJson: Coding.encode(sent)))
            if res.document == sent {
                base = sent
            } else if v == version {
                doc = res.document; base = res.document
                if res.conflicts > 0 {
                    conflictNotice = "Another device edited the same text. Your version is kept; theirs is in a callout at the end."
                }
            } else {
                base = sent // still typing: the next save merges the rest
            }
            if v == version { dirty = false }
            saveError = nil
            core.syncSoon()
        } catch { saveError = "\(error)" }
    }

    /// Another device changed this page.
    func remoteChanged() {
        if dirty { save(); return }
        guard let fresh = try? Coding.decode(Document.self, try core.loadDocumentJson(id: pageId)) else { return }
        if fresh != base { doc = fresh; base = fresh }
    }
}
