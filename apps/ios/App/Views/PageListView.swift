import SwiftUI

struct PageNode: Identifiable {
    let page: Page
    var children: [PageNode]?
    var id: String { page.id }
}

struct PageListView: View {
    @Environment(AppModel.self) private var model
    @Binding var selection: String?
    @Binding var query: String
    @Binding var showSync: Bool
    @State private var renaming: Page?
    @State private var newTitle = ""
    @State private var showArchive = false

    private func nodes(_ parent: String?) -> [PageNode] {
        model.children(of: parent).map { p in
            let kids = nodes(p.id)
            return PageNode(page: p, children: kids.isEmpty ? nil : kids)
        }
    }

    var body: some View {
        let hits = model.search(query)
        List(selection: $selection) {
            if !query.isEmpty {
                ForEach(hits) { hit in
                    VStack(alignment: .leading) {
                        Text(hit.page.label)
                        Text(hit.snippet.replacingOccurrences(of: "\u{1}", with: "").replacingOccurrences(of: "\u{2}", with: ""))
                            .font(.caption).foregroundStyle(.secondary).lineLimit(2)
                    }.tag(hit.page.id)
                }
                if hits.isEmpty { Text("No results").foregroundStyle(.secondary) }
            } else {
                OutlineGroup(nodes(nil), children: \.children) { node in
                    row(node.page).tag(node.page.id)
                }
            }
        }
        .searchable(text: $query, prompt: "Search pages")
        .overlay {
            if model.pages.filter({ !$0.isArchived }).isEmpty && query.isEmpty {
                ContentUnavailableView("No pages yet", systemImage: "square.and.pencil",
                                       description: Text("Create a page, or connect to a workspace to get your pages from another device."))
            }
        }
        .navigationTitle("Notercrux")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Button { showSync = true } label: { SyncBadge() }.accessibilityIdentifier("sync-button")
            }
            ToolbarItem(placement: .topBarTrailing) {
                Menu {
                    Button("Archive", systemImage: "archivebox") { showArchive = true }
                } label: { Image(systemName: "ellipsis.circle") }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    if let p = model.createPage(parent: nil) { selection = p.id }
                } label: { Image(systemName: "plus") }.accessibilityLabel("New page").accessibilityIdentifier("new-page")
            }
        }
        .sheet(isPresented: $showArchive) { ArchiveView() }
        .alert("Rename page", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
            TextField("Title", text: $newTitle)
            Button("Cancel", role: .cancel) {}
            Button("Rename") { if let r = renaming { model.rename(r.id, to: newTitle) } }
        }
    }

    private func row(_ p: Page) -> some View {
        Label(p.label, systemImage: "doc.text")
            .swipeActions(edge: .trailing) {
                Button("Archive", systemImage: "archivebox", role: .destructive) {
                    if selection == p.id { selection = nil }
                    model.archive(p.id)
                }
            }
            .contextMenu {
                Button("Rename", systemImage: "pencil") { newTitle = p.title; renaming = p }
                Button("New subpage", systemImage: "plus") { if let c = model.createPage(parent: p.id) { selection = c.id } }
                Button("Archive", systemImage: "archivebox", role: .destructive) {
                    if selection == p.id { selection = nil }
                    model.archive(p.id)
                }
            }
    }
}

struct ArchiveView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                ForEach(model.archived) { p in
                    HStack {
                        Text(p.label)
                        Spacer()
                        Button("Restore") { model.restore(p.id) }.buttonStyle(.bordered)
                    }
                    .swipeActions {
                        Button("Delete", role: .destructive) { model.deletePermanently(p.id) }
                    }
                }
                if model.archived.isEmpty { Text("Nothing archived").foregroundStyle(.secondary) }
            }
            .navigationTitle("Archive")
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
    }
}
