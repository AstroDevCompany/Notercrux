import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: String?
    @State private var showSync = DebugLaunch.invite != nil
    @State private var query = ""

    var body: some View {
        NavigationSplitView {
            PageListView(selection: $selection, query: $query, showSync: $showSync)
        } detail: {
            if let id = selection, let page = model.pages.first(where: { $0.id == id }) {
                PageView(page: page).id(id)
            } else {
                ContentUnavailableView("No page selected", systemImage: "doc.text",
                                       description: Text("Pick a page, or create one with the + button."))
            }
        }
        .sheet(isPresented: $showSync) { SyncView() }
        .alert("Heads up", isPresented: Binding(get: { model.notice != nil }, set: { if !$0 { model.notice = nil } })) {
            Button("OK") { model.notice = nil }
        } message: { Text(model.notice ?? "") }
        .alert("Something went wrong", isPresented: Binding(get: { model.error != nil }, set: { if !$0 { model.error = nil } })) {
            Button("OK") { model.error = nil }
        } message: { Text(model.error ?? "") }
    }
}

struct SyncBadge: View {
    @Environment(AppModel.self) private var model
    var body: some View {
        let s = model.status
        HStack(spacing: 4) {
            Image(systemName: icon(s.phase)).symbolEffect(.pulse, isActive: s.phase == "syncing")
            if s.pendingOps > 0 && s.phase != "disabled" { Text("\(s.pendingOps)").font(.caption2) }
        }
        .foregroundStyle(color(s.phase))
        .accessibilityLabel("Sync: \(label(s.phase))")
    }

    private func icon(_ p: String) -> String {
        switch p {
        case "idle": "checkmark.icloud"
        case "syncing": "arrow.triangle.2.circlepath.icloud"
        case "offline": "icloud.slash"
        case "error", "revoked", "keysMissing": "exclamationmark.icloud"
        default: "icloud"
        }
    }
    private func color(_ p: String) -> Color {
        switch p {
        case "error", "revoked", "keysMissing": .red
        case "offline": .orange
        case "disabled": .secondary
        default: .accentColor
        }
    }
    private func label(_ p: String) -> String {
        ["disabled": "off", "idle": "up to date", "syncing": "syncing", "offline": "offline",
         "error": "problem", "revoked": "removed", "keysMissing": "keys missing"][p] ?? p
    }
}
