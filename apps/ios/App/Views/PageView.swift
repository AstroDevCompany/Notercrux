import SwiftUI
import UIKit

struct PageView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.scenePhase) private var phase
    let page: Page
    @State private var editor: PageEditor?
    @State private var title = ""
    @FocusState private var titleFocused: Bool

    var body: some View {
        Group {
            if let editor {
                PageBody(editor: editor, title: $title, titleFocused: $titleFocused)
            } else {
                ProgressView()
            }
        }
        .navigationTitle(page.label)
        .navigationBarTitleDisplayMode(.inline)
        .onAppear {
            title = page.title
            if editor == nil { editor = PageEditor(pageId: page.id, core: model.core) }
        }
        .onDisappear { editor?.save() }
        .onChange(of: model.changeTick) { _, _ in
            if model.changedDocuments.contains("page:\(page.id)") { editor?.remoteChanged() }
        }
        .onChange(of: phase) { _, p in if p != .active { editor?.save() } }
    }
}

private struct PageBody: View {
    @Environment(AppModel.self) private var model
    @Bindable var editor: PageEditor
    @Binding var title: String
    var titleFocused: FocusState<Bool>.Binding
    @State private var titleSaver: Task<Void, Never>?
    @FocusState private var focused: String?

    var body: some View {
        List {
            Section {
                TextField("Untitled", text: $title)
                    .font(.largeTitle.bold())
                    .focused(titleFocused)
                    .listRowSeparator(.hidden)
                    .submitLabel(.next)
                    .onSubmit { editor.focus = editor.doc.blocks.first?.id ?? editor.addBlock(after: nil) }
                    // The title is saved as it is typed; nothing needs to be confirmed.
                    .onChange(of: title) { _, new in
                        titleSaver?.cancel()
                        titleSaver = Task {
                            try? await Task.sleep(for: .milliseconds(350))
                            if !Task.isCancelled { model.rename(editor.pageId, to: new) }
                        }
                    }
                    .onDisappear { titleSaver?.cancel(); model.rename(editor.pageId, to: title) }
            }
            .listRowBackground(Color.clear)
            Section {
                let rows = BlockOps.rows(editor.doc.blocks)
                ForEach(rows) { row in
                    BlockRow(row: row, editor: editor, focused: $focused)
                        .listRowSeparator(.hidden)
                }
                Button { editor.addBlock(after: editor.doc.blocks.last?.id) } label: {
                    Label("Add a block", systemImage: "plus")
                }
                .buttonStyle(.borderless).foregroundStyle(.secondary).listRowSeparator(.hidden)
                .accessibilityIdentifier("add-block")
            }
            .listRowBackground(Color.clear)
        }
        .listStyle(.plain)
        .scrollDismissesKeyboard(.interactively)
        .onChange(of: editor.focus) { _, id in if let id { focused = id; editor.focus = nil } }
        .toolbar {
            ToolbarItemGroup(placement: .keyboard) {
                if let id = focused, let b = editor.block(id) {
                    Menu {
                        ForEach(kinds, id: \.0) { k in Button(k.1) { editor.turn(id, into: k.0, level: k.2) } }
                    } label: { Image(systemName: "textformat") }
                    if b.type == "todo" {
                        Button { editor.toggleChecked(id) } label: { Image(systemName: b.checked ? "checkmark.square" : "square") }
                    }
                    Spacer()
                    Button { editor.addBlock(after: id) } label: { Image(systemName: "plus") }
                    Button("Done") { focused = nil }
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            if let e = editor.saveError {
                Text("Could not save: \(e)").font(.caption).padding(8).frame(maxWidth: .infinity).background(.red.opacity(0.15))
            }
        }
        .alert("Edited on two devices", isPresented: Binding(get: { editor.conflictNotice != nil }, set: { if !$0 { editor.conflictNotice = nil } })) {
            Button("OK") { editor.conflictNotice = nil }
        } message: { Text(editor.conflictNotice ?? "") }
    }

    private var kinds: [(String, String, Int?)] {
        [("paragraph", "Text", nil), ("heading", "Heading 1", 1), ("heading", "Heading 2", 2), ("heading", "Heading 3", 3),
         ("bulletedListItem", "Bulleted list", nil), ("numberedListItem", "Numbered list", nil), ("todo", "To-do", nil),
         ("quote", "Quote", nil), ("callout", "Callout", nil), ("code", "Code", nil)]
    }
}
