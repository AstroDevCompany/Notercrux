import SwiftUI
import UIKit

struct BlockRow: View {
    let row: BlockOps.Row
    @Bindable var editor: PageEditor
    var focused: FocusState<String?>.Binding
    @Environment(AppModel.self) private var model

    private var block: Block { row.block }
    /// Every text field starts with an invisible anchor character. Backspace at the start of a block
    /// deletes it, and that deletion is how the app learns Backspace was pressed there (a SwiftUI text
    /// field otherwise reports nothing when it is already empty).
    static let anchor = "\u{200B}"

    private var text: Binding<String> {
        Binding(get: { Self.anchor + (editor.block(block.id)?.text ?? block.text) },
                set: { new in
                    let current = editor.block(block.id)?.text ?? ""
                    if new == current, !new.contains(Self.anchor) {
                        editor.backspaceAtStart(block.id)
                    } else {
                        editor.setText(block.id, new.replacingOccurrences(of: Self.anchor, with: ""))
                    }
                })
    }

    private var slash: [BlockOps.SlashItem] {
        block.type == "code" ? [] : BlockOps.slashMatches(for: block.text)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                content
            }
            if !slash.isEmpty && focused.wrappedValue == block.id {
                SlashMenu(items: slash) { editor.applySlash(block.id, $0) }
            }
        }
        .padding(.leading, CGFloat(row.depth) * 20)
        .contextMenu {
            Menu("Turn into") {
                Button("Text") { editor.turn(block.id, into: "paragraph") }
                Button("Heading 1") { editor.turn(block.id, into: "heading", level: 1) }
                Button("Heading 2") { editor.turn(block.id, into: "heading", level: 2) }
                Button("Bulleted list") { editor.turn(block.id, into: "bulletedListItem") }
                Button("Numbered list") { editor.turn(block.id, into: "numberedListItem") }
                Button("To-do") { editor.turn(block.id, into: "todo") }
                Button("Quote") { editor.turn(block.id, into: "quote") }
                Button("Callout") { editor.turn(block.id, into: "callout") }
                Button("Code") { editor.turn(block.id, into: "code") }
            }
            Button("Move up", systemImage: "arrow.up") { editor.move(block.id, by: -1) }
            Button("Move down", systemImage: "arrow.down") { editor.move(block.id, by: 1) }
            Button("Duplicate", systemImage: "plus.square.on.square") { editor.duplicate(block.id) }
            Button("Delete", systemImage: "trash", role: .destructive) { editor.delete(block.id) }
        }
        .swipeActions(edge: .trailing) {
            Button("Delete", role: .destructive) { editor.delete(block.id) }
        }
    }

    @ViewBuilder private var content: some View {
        switch block.type {
        case "divider":
            Divider().padding(.vertical, 6)
        case "image":
            ImageBlockView(block: block, editor: editor)
        case "file":
            FileBlockView(block: block)
        case "heading":
            field.font(block.level == 1 ? .title.bold() : block.level == 2 ? .title2.bold() : .title3.bold())
        case "bulletedListItem":
            Text("•").foregroundStyle(.secondary); field
        case "numberedListItem":
            Text("\(row.number ?? 1).").foregroundStyle(.secondary).monospacedDigit(); field
        case "todo":
            Button { editor.toggleChecked(block.id) } label: {
                Image(systemName: block.checked ? "checkmark.square.fill" : "square")
            }.buttonStyle(.borderless)
            field.strikethrough(block.checked).foregroundStyle(block.checked ? .secondary : .primary)
        case "quote":
            Rectangle().fill(Color.accentColor.opacity(0.6)).frame(width: 3)
            field.italic()
        case "callout":
            Text(block.attrs?["icon"]?.stringValue ?? "💡")
            field
        case "code":
            field.font(.system(.callout, design: .monospaced))
                .padding(8).background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
        default:
            field
        }
    }

    private var field: some View {
        TextField(placeholder, text: text, axis: .vertical)
            .focused(focused, equals: block.id)
            .textInputAutocapitalization(block.type == "code" ? .never : .sentences)
            .autocorrectionDisabled(block.type == "code")
            .accessibilityIdentifier("block-\(block.id)")
    }

    private var placeholder: String { block.type == "heading" ? "Heading" : block.type == "paragraph" ? "Type something" : "" }
}

struct ImageBlockView: View {
    let block: Block
    @Bindable var editor: PageEditor
    @Environment(AppModel.self) private var model
    @State private var image: UIImage?
    @State private var meta: AttachmentMeta?

    private var id: String? { block.attrs?["attachment"]?.stringValue }

    var body: some View {
        VStack(spacing: 6) {
            if let image {
                Image(uiImage: image).resizable().scaledToFit().clipShape(RoundedRectangle(cornerRadius: 8))
            } else {
                Label(meta?.state == "failed" ? "This image could not be downloaded" : "Waiting for this image to download…",
                      systemImage: meta?.state == "failed" ? "exclamationmark.triangle" : "arrow.down.circle")
                    .foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(24)
                    .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
            }
            TextField("Add a caption", text: Binding(get: { editor.block(block.id)?.text ?? "" }, set: { editor.setCaption(block.id, $0) }))
                .font(.caption).multilineTextAlignment(.center)
        }
        .task(id: model.status.attachmentsPending) { await load() }
        .task { await load() }
    }

    private func load() async {
        guard let id else { return }
        meta = (try? model.core.attachmentJson(id: id)).flatMap { $0 }.flatMap { try? Coding.decode(AttachmentMeta.self, $0) }
        if image == nil, let data = try? model.core.attachmentData(id: id), let img = UIImage(data: data) { image = img }
    }
}

struct FileBlockView: View {
    let block: Block
    var body: some View {
        Label {
            VStack(alignment: .leading) {
                Text(block.attrs?["name"]?.stringValue ?? "File")
                Text(ByteCountFormatter.string(fromByteCount: Int64(block.attrs?["size"]?.intValue ?? 0), countStyle: .file))
                    .font(.caption).foregroundStyle(.secondary)
            }
        } icon: { Image(systemName: "doc") }
        .padding(10).frame(maxWidth: .infinity, alignment: .leading)
        .background(.quaternary, in: RoundedRectangle(cornerRadius: 8))
    }
}

/// The "/" command list, shown under the block being typed in.
struct SlashMenu: View {
    let items: [BlockOps.SlashItem]
    let pick: (BlockOps.SlashItem) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(items.prefix(6)) { item in
                Button { pick(item) } label: {
                    Text(item.title).frame(maxWidth: .infinity, alignment: .leading).padding(.vertical, 8).padding(.horizontal, 12)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("slash-\(item.id)")
                if item.id != items.prefix(6).last?.id { Divider() }
            }
        }
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(.separator))
    }
}
