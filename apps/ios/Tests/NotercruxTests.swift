import XCTest
@testable import Notercrux

final class BlockOpsTests: XCTestCase {
    func testMarksFollowTextInsertedBefore() {
        let marks = [Mark(from: 6, to: 11, type: "bold", attrs: nil)]
        let out = BlockOps.adjust(marks: marks, old: "hello world", new: "oh hello world")
        XCTAssertEqual(out, [Mark(from: 9, to: 14, type: "bold", attrs: nil)])
    }

    func testTypingInsideAndAtTheEndOfBoldExtendsIt() {
        let marks = [Mark(from: 0, to: 5, type: "bold", attrs: nil)]
        XCTAssertEqual(BlockOps.adjust(marks: marks, old: "hello", new: "helXlo")?.first?.to, 6)
        XCTAssertEqual(BlockOps.adjust(marks: marks, old: "hello", new: "hello!")?.first?.to, 6)
        let link = [Mark(from: 0, to: 5, type: "link", attrs: ["href": .string("x")])]
        XCTAssertEqual(BlockOps.adjust(marks: link, old: "hello", new: "hello!")?.first?.to, 5, "links do not grow")
    }

    func testDeletingMarkedTextRemovesTheMark() {
        let marks = [Mark(from: 6, to: 11, type: "italic", attrs: nil)]
        XCTAssertNil(BlockOps.adjust(marks: marks, old: "hello world", new: "hello "))
    }

    func testOffsetsCountUnicodeScalarsNotCharacters() {
        // "🇮🇹" is two scalars, one Character.
        let marks = [Mark(from: 3, to: 6, type: "bold", attrs: nil)]
        let out = BlockOps.adjust(marks: marks, old: "🇮🇹 abc", new: "x🇮🇹 abc")
        XCTAssertEqual(out, [Mark(from: 4, to: 7, type: "bold", attrs: nil)])
    }

    func testSplitDividesTextAndMarks() {
        var b = Block(id: "a", type: "paragraph", text: "hello world")
        b.marks = [Mark(from: 0, to: 11, type: "bold", attrs: nil)]
        let (head, tail) = BlockOps.split(b, at: 5)
        XCTAssertEqual(head.text, "hello")
        XCTAssertEqual(tail.text, "world")
        XCTAssertEqual(head.marks, [Mark(from: 0, to: 5, type: "bold", attrs: nil)])
        XCTAssertEqual(tail.marks, [Mark(from: 0, to: 5, type: "bold", attrs: nil)])
        XCTAssertNotEqual(head.id, tail.id)
    }

    func testRowsNumberConsecutiveListItemsAndIndentChildren() {
        var parent = Block(id: "p", type: "numberedListItem", text: "one")
        parent.children = [Block(id: "c", type: "bulletedListItem", text: "child")]
        let rows = BlockOps.rows([parent, Block(id: "q", type: "numberedListItem", text: "two"), Block(id: "r", text: "x"), Block(id: "s", type: "numberedListItem", text: "again")])
        XCTAssertEqual(rows.map(\.number), [1, nil, 2, nil, 1])
        XCTAssertEqual(rows.map(\.depth), [0, 1, 0, 0, 0])
    }

    func testTurnIntoSetsAndClearsKindSpecificAttributes() {
        var b = Block(type: "heading", text: "t", attrs: ["level": .int(2)])
        BlockOps.turn(&b, into: "todo")
        XCTAssertEqual(b.attrs?["checked"], .bool(false))
        XCTAssertNil(b.attrs?["level"])
        BlockOps.turn(&b, into: "paragraph")
        XCTAssertNil(b.attrs)
    }
}

final class ContractTests: XCTestCase {
    /// A document as the desktop app writes it, including things this app does not edit.
    let desktopJson = """
    {"version":1,"blocks":[
      {"id":"b1","type":"heading","text":"Plan","attrs":{"level":2}},
      {"id":"b2","type":"paragraph","text":"see Roadmap","marks":[{"from":4,"to":11,"type":"pageLink","attrs":{"pageId":"0190a000-0000-7000-8000-000000000001"}}]},
      {"id":"b3","type":"image","text":"caption","attrs":{"attachment":"\(String(repeating: "a", count: 64))","name":"cat.png","mime":"image/png","size":4096,"future":{"x":[1,2.5,null,true]}}},
      {"id":"b4","type":"someFutureBlock","text":"kept","attrs":{"k":"v"}}
    ]}
    """

    func testDocumentsRoundTripWithoutLosingUnknownContent() throws {
        let doc = try Coding.decode(Document.self, desktopJson)
        XCTAssertEqual(doc.blocks.count, 4)
        XCTAssertEqual(doc.blocks[0].level, 2)
        let again = try Coding.decode(Document.self, try Coding.encode(doc))
        XCTAssertEqual(again, doc)
        XCTAssertEqual(again.blocks[2].attrs?["future"], .object(["x": .array([.int(1), .double(2.5), .null, .bool(true)])]))
        XCTAssertEqual(again.blocks[3].type, "someFutureBlock")
        XCTAssertEqual(again.blocks[1].marks?.first?.attrs?["pageId"]?.stringValue, "0190a000-0000-7000-8000-000000000001")
    }

    func testIntegersStayIntegersInJson() throws {
        let doc = try Coding.decode(Document.self, desktopJson)
        let text = try Coding.encode(doc)
        XCTAssertTrue(text.contains("\"size\":4096"), text)
        XCTAssertFalse(text.contains("4096.0"))
    }
}

@MainActor
final class CoreIntegrationTests: XCTestCase {
    func makeModel() throws -> AppModel {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("ncx-\(UUID().uuidString)")
        return AppModel(core: try Core(dataDir: dir.path, vault: MemoryVault()))
    }

    func testPagesDocumentsAndRebasedSavesThroughTheRustCore() throws {
        let model = try makeModel()
        let page = try XCTUnwrap(model.createPage(parent: nil, title: "Groceries"))
        XCTAssertEqual(model.children(of: nil).map(\.title), ["Groceries"])

        let editor = PageEditor(pageId: page.id, core: model.core)
        let first = editor.addBlock(after: nil)
        editor.setText(first, "milk")
        editor.save()
        XCTAssertNil(editor.saveError)

        // A second editor on the same page (another window, or a remote change) sees it.
        let other = PageEditor(pageId: page.id, core: model.core)
        XCTAssertEqual(other.doc.blocks.map(\.text), ["milk"])

        // Both edit from the same base; the second save keeps the first one's change.
        editor.setText(first, "milk, eggs")
        other.addBlock(after: first)
        editor.save()
        other.save()
        let final = try Coding.decode(Document.self, try model.core.loadDocumentJson(id: page.id))
        XCTAssertEqual(final.blocks.count, 2)
        XCTAssertEqual(final.blocks[0].text, "milk, eggs")
        XCTAssertEqual(other.doc, final, "the editor adopts the merged page")
    }

    func testReturnSplitsBlocksAndLeavesEmptyListItems() throws {
        let model = try makeModel()
        let page = try XCTUnwrap(model.createPage(parent: nil, title: "T"))
        let e = PageEditor(pageId: page.id, core: model.core)
        let id = e.addBlock(after: nil, kind: "bulletedListItem")
        e.setText(id, "one\ntwo")
        XCTAssertEqual(e.doc.blocks.map(\.text), ["one", "two"])
        XCTAssertEqual(e.doc.blocks.map(\.type), ["bulletedListItem", "bulletedListItem"])
        XCTAssertEqual(e.focus, e.doc.blocks[1].id)
        // Return on an empty item turns it into a paragraph instead of adding another.
        let empty = e.doc.blocks[1].id
        e.setText(empty, "\n")
        XCTAssertEqual(e.doc.blocks.count, 2)
        XCTAssertEqual(e.doc.blocks[1].type, "paragraph")
        // Code blocks keep their newlines.
        let code = e.addBlock(after: empty, kind: "code")
        e.setText(code, "a\nb")
        XCTAssertEqual(e.block(code)?.text, "a\nb")
    }

    func testArchiveRestoreAndSearch() throws {
        let model = try makeModel()
        let p = try XCTUnwrap(model.createPage(parent: nil, title: "Searchable thing"))
        XCTAssertEqual(model.search("searchable").map(\.page.id), [p.id])
        model.archive(p.id)
        XCTAssertTrue(model.children(of: nil).isEmpty)
        XCTAssertEqual(model.archived.count, 1)
        model.restore(p.id)
        XCTAssertEqual(model.children(of: nil).count, 1)
    }

    /// Regression: async calls used to panic ("no reactor running") when made from Swift.
    func testAsyncCallsRunOnTheCoresRuntime() async throws {
        let model = try makeModel()
        do {
            _ = try await model.core.beginJoin(code: "NCX1-notvalid", label: "Test phone")
            XCTFail("an invalid code must be rejected")
        } catch CoreError.Message(let m) {
            XCTAssertTrue(m.contains("not valid"), m)
        }
        do {
            _ = try await model.core.pollJoin(mode: "keepLocal")
            XCTFail("no join is in progress")
        } catch CoreError.Message {}
    }

    func testBackspaceOnEmptyBlocksLeavesTheListThenDeletesAndFocusesThePreviousBlock() throws {
        let model = try makeModel()
        let page = try XCTUnwrap(model.createPage(parent: nil, title: "T"))
        let e = PageEditor(pageId: page.id, core: model.core)
        let a = e.addBlock(after: nil)
        e.setText(a, "first")
        let b = e.addBlock(after: a, kind: "bulletedListItem")
        e.backspaceAtStart(b, settle: false)
        XCTAssertEqual(e.block(b)?.type, "paragraph", "a list item first becomes a paragraph")
        e.backspaceAtStart(b, settle: false)
        XCTAssertNil(e.block(b), "then the empty block is removed")
        XCTAssertEqual(e.focus, a, "focus moves to the previous block")
        // The last remaining block can be emptied but a lone empty paragraph is deleted too.
        XCTAssertEqual(e.doc.blocks.map(\.text), ["first"])
    }

    func testBackspaceAtTheStartOfTextMergesIntoThePreviousBlock() throws {
        let model = try makeModel()
        let page = try XCTUnwrap(model.createPage(parent: nil, title: "T"))
        let e = PageEditor(pageId: page.id, core: model.core)
        let a = e.addBlock(after: nil)
        e.setText(a, "hello ")
        let b = e.addBlock(after: a)
        e.setText(b, "world")
        e.backspaceAtStart(b, settle: false)
        XCTAssertEqual(e.doc.blocks.map(\.text), ["hello world"])
        XCTAssertEqual(e.focus, a)
    }

    func testSlashCommandsFilterAndTurnTheBlock() throws {
        XCTAssertEqual(BlockOps.slashMatches(for: "/").count, BlockOps.slashItems.count)
        XCTAssertEqual(BlockOps.slashMatches(for: "/head").map(\.id), ["h1", "h2", "h3"], "typing a title prefix finds the headings")
        XCTAssertTrue(BlockOps.slashMatches(for: "/todo").contains { $0.kind == "todo" })
        XCTAssertTrue(BlockOps.slashMatches(for: "/code ").isEmpty, "a space ends the command")
        XCTAssertTrue(BlockOps.slashMatches(for: "plain").isEmpty)
        XCTAssertTrue(BlockOps.slashMatches(for: "/zzz").isEmpty)

        let model = try makeModel()
        let page = try XCTUnwrap(model.createPage(parent: nil, title: "T"))
        let e = PageEditor(pageId: page.id, core: model.core)
        let id = e.addBlock(after: nil)
        e.setText(id, "/to")
        e.applySlash(id, try XCTUnwrap(BlockOps.slashMatches(for: "/to").first { $0.id == "todo" }))
        XCTAssertEqual(e.block(id)?.type, "todo")
        XCTAssertEqual(e.block(id)?.text, "")
        // A divider is followed by a fresh paragraph to keep typing in.
        e.setText(id, "/div")
        e.applySlash(id, try XCTUnwrap(BlockOps.slashItems.first { $0.id == "divider" }))
        XCTAssertEqual(e.doc.blocks.map(\.type), ["divider", "paragraph"])
    }

    func testBlocksMoveUpAndDown() throws {
        var blocks = [Block(id: "a", text: "1"), Block(id: "b", text: "2"), Block(id: "c", text: "3")]
        XCTAssertTrue(BlockOps.move(&blocks, id: "c", by: -1))
        XCTAssertEqual(blocks.map(\.id), ["a", "c", "b"])
        XCTAssertFalse(BlockOps.move(&blocks, id: "a", by: -1))
    }

    func testBadInputsBecomeErrorsNotCrashes() throws {
        let model = try makeModel()
        XCTAssertThrowsError(try model.core.loadDocumentJson(id: "nope"))
        XCTAssertEqual(model.status.phase, "disabled")
    }
}
