import Foundation
import Security

/// Device keys and the workspace key ring live in the iOS Keychain, readable only on this device
/// after its first unlock, and never included in backups or synced to iCloud.
final class KeychainVault: SecretVault, @unchecked Sendable {
    private let service: String

    init(service: String = "app.notercrux.ios") { self.service = service }

    private func query(_ name: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: name]
    }

    func get(name: String) -> Data? {
        var q = query(name)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: AnyObject?
        return SecItemCopyMatching(q as CFDictionary, &out) == errSecSuccess ? out as? Data : nil
    }

    func set(name: String, value: Data) -> Bool {
        var attrs = query(name)
        let update: [String: Any] = [kSecValueData as String: value]
        let status = SecItemUpdate(attrs as CFDictionary, update as CFDictionary)
        if status == errSecSuccess { return true }
        guard status == errSecItemNotFound else { return false }
        attrs[kSecValueData as String] = value
        attrs[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        return SecItemAdd(attrs as CFDictionary, nil) == errSecSuccess
    }

    func delete(name: String) -> Bool {
        let s = SecItemDelete(query(name) as CFDictionary)
        return s == errSecSuccess || s == errSecItemNotFound
    }
}

/// Keeps secrets in memory (tests).
final class MemoryVault: SecretVault, @unchecked Sendable {
    private var items: [String: Data] = [:]
    private let lock = NSLock()
    func get(name: String) -> Data? { lock.lock(); defer { lock.unlock() }; return items[name] }
    func set(name: String, value: Data) -> Bool { lock.lock(); defer { lock.unlock() }; items[name] = value; return true }
    func delete(name: String) -> Bool { lock.lock(); defer { lock.unlock() }; items[name] = nil; return true }
}
