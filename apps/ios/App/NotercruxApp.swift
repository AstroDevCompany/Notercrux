import SwiftUI

@main
struct NotercruxApp: App {
    @State private var model: AppModel?
    @State private var startupError: String?
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            Group {
                if let model {
                    RootView().environment(model)
                } else if let startupError {
                    ContentUnavailableView("Notercrux cannot start", systemImage: "exclamationmark.triangle",
                                           description: Text(startupError))
                } else {
                    ProgressView()
                }
            }
            .task {
                guard model == nil else { return }
                do {
                    let m = try AppModel.live()
                    m.startSync()
                    model = m
                } catch { startupError = "\(error)" }
            }
            .onChange(of: phase) { _, new in
                guard let model else { return }
                if new == .active { model.syncNow() }
                if new == .background { model.finishSyncInBackground() }
            }
        }
    }
}
