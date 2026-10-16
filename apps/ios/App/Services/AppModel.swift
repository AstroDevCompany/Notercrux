import Foundation
import Observation
import UIKit

/// Everything the views share: the Rust core, the page list and sync state.
@MainActor
@Observable
final class AppModel {
    let core: Core
    private(set) var pages: [Page] = []
    private(set) var status = SyncStatus()
    private(set) var events: [SyncEvent] = []
    /// Bumps whenever another device's changes were applied; editors watch it.
    private(set) var changeTick = 0
    private(set) var changedDocuments: Set<String> = []
    var notice: String?
    var error: String?

    private var listener: Listener?

    init(core: Core) {
        self.core = core
        reloadPages()
        refreshStatus()
    }

    static func live(dataDir: URL? = nil) throws -> AppModel {
        let dir = try dataDir ?? FileManager.default
            .url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
            .appendingPathComponent("Notercrux", isDirectory: true)
        let core = try Core(dataDir: dir.path, vault: KeychainVault())
        return AppModel(core: core)
    }

    // ---- pages ------------------------------------------------------------------------------

    func reloadPages() {
        do {
            pages = try Coding.decode([Page].self, try core.listPagesJson())
        } catch { fail(error) }
    }

    func children(of parent: String?) -> [Page] {
        pages.filter { $0.parentId == parent && !$0.isArchived && $0.kind == "page" }
            .sorted { $0.position < $1.position }
    }

    var archived: [Page] { pages.filter { $0.isArchived } }

    @discardableResult
    func createPage(parent: String?, title: String = "") -> Page? {
        do {
            let p = try Coding.decode(Page.self, try core.createPageJson(parentId: parent, title: title))
            reloadPages()
            core.syncSoon()
            return p
        } catch { fail(error); return nil }
    }

    func rename(_ id: String, to title: String) {
        run { try core.renamePage(id: id, title: title) }
    }
    func archive(_ id: String) { run { try core.archivePage(id: id) } }
    func restore(_ id: String) { run { try core.restorePage(id: id) } }
    func deletePermanently(_ id: String) { run { try core.deletePage(id: id) } }

    func search(_ q: String) -> [SearchHit] {
        guard !q.trimmingCharacters(in: .whitespaces).isEmpty else { return [] }
        return (try? Coding.decode([SearchHit].self, try core.searchJson(query: q))) ?? []
    }

    private func run(_ f: () throws -> Void) {
        do { try f(); reloadPages(); core.syncSoon() } catch { fail(error) }
    }

    func fail(_ e: Error) { error = "\(e)".replacingOccurrences(of: "Message(message: ", with: "").trimmingCharacters(in: CharacterSet(charactersIn: "\"()")) }

    // ---- sync -------------------------------------------------------------------------------

    func startSync() {
        guard listener == nil else { return }
        let l = Listener(model: self)
        listener = l
        core.startSync(listener: l)
    }

    func refreshStatus() {
        if let s = try? Coding.decode(SyncStatus.self, try core.syncStatusJson()) { status = s }
        if let e = try? Coding.decode([SyncEvent].self, try core.syncEventsJson()) { events = e }
    }

    fileprivate func apply(status json: String) {
        #if DEBUG
        print("NCX status \(json.prefix(160))")
        #endif
        if let s = try? Coding.decode(SyncStatus.self, json) { status = s }
        if let e = try? Coding.decode([SyncEvent].self, (try? core.syncEventsJson()) ?? "[]") { events = e }
    }

    fileprivate func apply(report json: String) {
        guard let r = try? Coding.decode(ApplyReport.self, json) else { return }
        if r.pagesChanged { reloadPages() }
        changedDocuments = Set(r.documentsChanged)
        changeTick += 1
        if r.recoveredBlocks > 0 {
            notice = "Another device deleted text you were editing. It was restored in a callout at the end of the page."
        }
    }

    func syncNow() { core.syncSoon() }

    /// Gives a sync in flight a little time to finish when the app leaves the foreground.
    func finishSyncInBackground() {
        core.syncSoon()
        var task = UIBackgroundTaskIdentifier.invalid
        task = UIApplication.shared.beginBackgroundTask(withName: "notercrux-sync") {
            UIApplication.shared.endBackgroundTask(task)
        }
        Task {
            try? await Task.sleep(for: .seconds(8))
            UIApplication.shared.endBackgroundTask(task)
        }
    }

    func afterLinkChange() {
        refreshStatus()
        reloadPages()
    }

    private final class Listener: CoreListener, @unchecked Sendable {
        weak var model: AppModel?
        init(model: AppModel) { self.model = model }
        func statusChanged(statusJson: String) {
            Task { @MainActor in self.model?.apply(status: statusJson) }
        }
        func dataChanged(reportJson: String) {
            Task { @MainActor in self.model?.apply(report: reportJson) }
        }
    }
}
