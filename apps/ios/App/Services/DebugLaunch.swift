import Foundation

/// Launch-time hooks for testing on a device. They do nothing in release builds.
enum DebugLaunch {
    static var invite: String? {
        #if DEBUG
        let v = ProcessInfo.processInfo.environment["NCX_INVITE"]
        return (v?.isEmpty == false) ? v : nil
        #else
        return nil
        #endif
    }
}
