import Foundation

/// These types mirror `crates/domain` and `packages/document-model`. The JSON is the contract.

struct Page: Codable, Identifiable, Equatable, Hashable {
    var id: String
    var parentId: String?
    var kind: String
    var title: String
    var icon: String?
    var position: Double
    var createdAt: Int64
    var updatedAt: Int64
    var archivedAt: Int64?

    var isArchived: Bool { archivedAt != nil }
    var label: String { title.trimmingCharacters(in: .whitespaces).isEmpty ? "Untitled" : title }
}

struct Mark: Codable, Equatable, Hashable {
    var from: Int
    var to: Int
    var type: String
    var attrs: [String: JSONValue]?
}

struct Block: Codable, Equatable, Identifiable, Hashable {
    var id: String
    var type: String
    var attrs: [String: JSONValue]?
    var text: String
    var marks: [Mark]?
    var children: [Block]?

    init(id: String = Block.newId(), type: String = "paragraph", text: String = "",
         attrs: [String: JSONValue]? = nil, marks: [Mark]? = nil, children: [Block]? = nil) {
        self.id = id; self.type = type; self.text = text
        self.attrs = attrs; self.marks = marks; self.children = children
    }

    static func newId() -> String { UUID().uuidString.lowercased() }

    var level: Int { attrs?["level"]?.intValue ?? 1 }
    var checked: Bool { attrs?["checked"]?.boolValue ?? false }
}

struct Document: Codable, Equatable {
    var version: Int = 1
    var blocks: [Block] = []
}

struct Rebased: Codable {
    var document: Document
    var conflicts: Int
}

struct SearchHit: Codable, Identifiable {
    var page: Page
    var snippet: String
    var id: String { page.id }
}

struct SyncStatus: Codable, Equatable {
    var phase: String = "disabled"
    var server: String?
    var workspaceId: String?
    var deviceId: String?
    var epoch: Int = 0
    var pendingOps: Int = 0
    var heldOps: Int = 0
    var lastSyncAt: Int64?
    var lastError: String?
    var retryInSecs: Int?
    var opsSent: Int = 0
    var opsReceived: Int = 0
    var rejectedOps: Int = 0
    var recoveredBlocks: Int = 0
    var attachmentsPending: Int = 0
    var attachmentsFailed: Int = 0

    var isLinked: Bool { phase != "disabled" && phase != "keysMissing" }
}

struct SyncEvent: Codable, Identifiable {
    var at: Int64
    var level: String
    var message: String
    var id: String { "\(at)-\(message)" }
}

struct DeviceView: Codable, Identifiable {
    var id: String
    var label: String
    var current: Bool
    var revoked: Bool
    var createdAt: Int64
    var lastSeen: Int64?
}

struct ApplyReport: Codable {
    var applied: Int = 0
    var held: Int = 0
    var recoveredBlocks: Int = 0
    var pagesChanged: Bool = false
    var documentsChanged: [String] = []
}

struct AttachmentMeta: Codable {
    var id: String
    var name: String
    var mime: String
    var size: Int64
    var state: String
    var error: String?
    var chunksPresent: Int
    var chunksTotal: Int
}

enum Coding {
    static let decoder = JSONDecoder()
    static let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.outputFormatting = [.sortedKeys]
        return e
    }()

    static func decode<T: Decodable>(_ type: T.Type, _ json: String) throws -> T {
        try decoder.decode(type, from: Data(json.utf8))
    }

    static func encode<T: Encodable>(_ value: T) throws -> String {
        String(decoding: try encoder.encode(value), as: UTF8.self)
    }
}
