import Foundation

/// Pure editing operations on blocks. Kept free of UI so they can be unit tested.
enum BlockOps {
    static let textKinds = ["paragraph", "heading", "bulletedListItem", "numberedListItem", "todo", "quote", "callout", "code", "pageLink"]

    /// Keeps inline formatting attached to the right characters after the text of a block changed.
    /// Offsets are Unicode scalar counts, like the rest of the document model.
    static func adjust(marks: [Mark]?, old: String, new: String) -> [Mark]? {
        guard let marks, !marks.isEmpty else { return nil }
        let o = Array(old.unicodeScalars), n = Array(new.unicodeScalars)
        var p = 0
        while p < min(o.count, n.count), o[p] == n[p] { p += 1 }
        var s = 0
        while s < min(o.count, n.count) - p, o[o.count - 1 - s] == n[n.count - 1 - s] { s += 1 }
        let removedEnd = o.count - s
        let inserted = n.count - p - s
        let delta = inserted - (removedEnd - p)
        let grows = ["bold", "italic", "strike", "underline"]
        var out: [Mark] = []
        for m in marks {
            var from = m.from, to = m.to
            if from >= removedEnd { from += delta } else if from > p { from = p }
            if to > removedEnd { to += delta }
            else if to > p { to = (m.from < p) ? p + inserted : p }
            else if to == p, removedEnd == p, grows.contains(m.type), inserted > 0 { to = p + inserted }
            from = max(0, min(from, n.count)); to = max(0, min(to, n.count))
            if to > from { var c = m; c.from = from; c.to = to; out.append(c) }
        }
        return out.isEmpty ? nil : out
    }

    /// Splits `block` at scalar offset `at` (the position of a typed newline, which is dropped).
    static func split(_ block: Block, at: Int) -> (head: Block, tail: Block) {
        let scalars = Array(block.text.unicodeScalars)
        let cut = min(max(at, 0), scalars.count)
        var head = block, tail = block
        head.text = String(String.UnicodeScalarView(scalars[..<cut]))
        tail.text = String(String.UnicodeScalarView(scalars[min(cut + 1, scalars.count)...]))
        head.marks = clip(block.marks, from: 0, to: cut)
        tail.marks = clip(block.marks, from: cut + 1, to: scalars.count)
        head.children = nil
        tail.id = Block.newId()
        tail.children = block.children
        return (head, tail)
    }

    private static func clip(_ marks: [Mark]?, from lo: Int, to hi: Int) -> [Mark]? {
        let out = (marks ?? []).compactMap { m -> Mark? in
            let f = max(m.from, lo), t = min(m.to, hi)
            guard t > f else { return nil }
            var c = m; c.from = f - lo; c.to = t - lo
            return c
        }
        return out.isEmpty ? nil : out
    }

    /// The kind a new block gets when Return is pressed in `kind`.
    static func continuation(of kind: String) -> String {
        ["bulletedListItem", "numberedListItem", "todo", "quote"].contains(kind) ? kind : "paragraph"
    }

    /// Return does not split these (a newline belongs to the content).
    static func keepsNewlines(_ kind: String) -> Bool { kind == "code" || kind == "callout" }

    static func turn(_ block: inout Block, into kind: String, level: Int? = nil) {
        block.type = kind
        var attrs = block.attrs ?? [:]
        attrs["level"] = nil
        attrs["checked"] = nil
        if kind == "heading" { attrs["level"] = .int(level ?? 1) }
        if kind == "todo" { attrs["checked"] = .bool(false) }
        if kind == "callout", attrs["icon"] == nil { attrs["icon"] = .string("💡") }
        block.attrs = attrs.isEmpty ? nil : attrs
        if kind == "code" { block.marks = nil }
    }

    /// Swaps `id` with its neighbour among siblings. Returns false at the edge.
    @discardableResult
    static func move(_ blocks: inout [Block], id: String, by delta: Int) -> Bool {
        if let i = blocks.firstIndex(where: { $0.id == id }) {
            let j = i + delta
            guard blocks.indices.contains(j) else { return false }
            blocks.swapAt(i, j)
            return true
        }
        for i in blocks.indices {
            if var kids = blocks[i].children, move(&kids, id: id, by: delta) {
                blocks[i].children = kids
                return true
            }
        }
        return false
    }

    /// Appends `tail` to `head` (Backspace at the start of a block). Marks of the tail shift right.
    static func merge(head: Block, tail: Block) -> Block {
        var out = head
        let offset = head.text.unicodeScalars.count
        out.text = head.text + tail.text
        let shifted = (tail.marks ?? []).map { m -> Mark in
            var c = m; c.from += offset; c.to += offset; return c
        }
        let all = (head.marks ?? []) + shifted
        out.marks = all.isEmpty ? nil : all
        if let kids = tail.children, !kids.isEmpty { out.children = (head.children ?? []) + kids }
        return out
    }

    /// Items of the "/" menu.
    struct SlashItem: Identifiable {
        let id: String
        let title: String
        let kind: String
        var level: Int? = nil
        var keywords: [String] = []
    }

    static let slashItems: [SlashItem] = [
        .init(id: "text", title: "Text", kind: "paragraph", keywords: ["paragraph", "p"]),
        .init(id: "h1", title: "Heading 1", kind: "heading", level: 1, keywords: ["title", "h1"]),
        .init(id: "h2", title: "Heading 2", kind: "heading", level: 2, keywords: ["h2"]),
        .init(id: "h3", title: "Heading 3", kind: "heading", level: 3, keywords: ["h3"]),
        .init(id: "bullet", title: "Bulleted list", kind: "bulletedListItem", keywords: ["ul", "list"]),
        .init(id: "number", title: "Numbered list", kind: "numberedListItem", keywords: ["ol", "list"]),
        .init(id: "todo", title: "To-do", kind: "todo", keywords: ["task", "checkbox"]),
        .init(id: "quote", title: "Quote", kind: "quote", keywords: ["blockquote"]),
        .init(id: "callout", title: "Callout", kind: "callout", keywords: ["note", "info"]),
        .init(id: "code", title: "Code", kind: "code", keywords: ["snippet"]),
        .init(id: "divider", title: "Divider", kind: "divider", keywords: ["line", "rule"]),
    ]

    /// The menu to show for a block whose text is `text`, or none when the text is not a command.
    static func slashMatches(for text: String) -> [SlashItem] {
        guard text.hasPrefix("/"), !text.contains(" "), !text.contains("\n") else { return [] }
        let q = text.dropFirst().lowercased()
        if q.isEmpty { return slashItems }
        return slashItems.filter { $0.title.lowercased().hasPrefix(q) || $0.keywords.contains { $0.hasPrefix(q) } || $0.id.hasPrefix(q) }
    }

    // ---- tree access ------------------------------------------------------------------------

    static func update(_ blocks: inout [Block], id: String, _ change: (inout Block) -> Void) -> Bool {
        for i in blocks.indices {
            if blocks[i].id == id { change(&blocks[i]); return true }
            if var kids = blocks[i].children, update(&kids, id: id, change) {
                blocks[i].children = kids
                return true
            }
        }
        return false
    }

    static func find(_ blocks: [Block], id: String) -> Block? {
        for b in blocks {
            if b.id == id { return b }
            if let f = find(b.children ?? [], id: id) { return f }
        }
        return nil
    }

    @discardableResult
    static func insert(_ blocks: inout [Block], _ new: Block, after id: String?) -> Bool {
        guard let id else { blocks.append(new); return true }
        for i in blocks.indices {
            if blocks[i].id == id { blocks.insert(new, at: i + 1); return true }
            if var kids = blocks[i].children, insert(&kids, new, after: id) {
                blocks[i].children = kids
                return true
            }
        }
        return false
    }

    @discardableResult
    static func remove(_ blocks: inout [Block], id: String) -> Bool {
        if let i = blocks.firstIndex(where: { $0.id == id }) { blocks.remove(at: i); return true }
        for i in blocks.indices {
            if var kids = blocks[i].children, remove(&kids, id: id) {
                blocks[i].children = kids.isEmpty ? nil : kids
                return true
            }
        }
        return false
    }

    /// Depth-first rows for display.
    struct Row: Identifiable { var block: Block; var depth: Int; var number: Int?; var id: String { block.id } }

    static func rows(_ blocks: [Block], depth: Int = 0) -> [Row] {
        var out: [Row] = []
        var counter = 0
        var prev = ""
        for b in blocks {
            counter = b.type == "numberedListItem" ? (prev == "numberedListItem" ? counter + 1 : 1) : 0
            prev = b.type
            out.append(Row(block: b, depth: depth, number: b.type == "numberedListItem" ? counter : nil))
            out += rows(b.children ?? [], depth: depth + 1)
        }
        return out
    }
}
