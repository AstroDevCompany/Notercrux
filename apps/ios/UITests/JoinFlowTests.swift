import XCTest

/// Drives the real app on a device or simulator against a running server and the `peer` example
/// (see crates/sync/examples/peer.rs). The invite code arrives in TEST_RUNNER_NCX_INVITE.
final class JoinFlowTests: XCTestCase {
    func testJoinWorkspaceThenEditAndSyncWithDesktopStandIn() throws {
        let code = try XCTUnwrap(ProcessInfo.processInfo.environment["NCX_INVITE"], "set TEST_RUNNER_NCX_INVITE")
        addUIInterruptionMonitor(withDescription: "Local network") { alert in
            for label in ["Allow", "OK"] where alert.buttons[label].exists {
                alert.buttons[label].tap(); return true
            }
            return false
        }
        let app = XCUIApplication()
        app.launch()
        func el(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id].firstMatch }

        // Nothing here yet.
        XCTAssertTrue(app.staticTexts["No pages yet"].waitForExistence(timeout: 20) || app.navigationBars.firstMatch.waitForExistence(timeout: 5))
        el("sync-button").tap()

        let field = el("invite-code")
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        field.typeText(code)
        app.buttons["continue"].firstMatch.tap()
        app.tap() // lets the local network prompt (if any) be handled
        let sas = el("sas")
        XCTAssertTrue(sas.waitForExistence(timeout: 30), "the request reaches the server and a code is shown")
        print("SAS_ON_PHONE=\(sas.label)")

        // The desktop stand-in approves; the phone joins and shows its status.
        XCTAssertTrue(app.buttons["Sync now"].waitForExistence(timeout: 90), "joined after approval")
        app.buttons["Done"].firstMatch.tap()

        // The workspace's pages arrive.
        let shared = app.staticTexts["Shared plans"]
        XCTAssertTrue(shared.waitForExistence(timeout: 60), "pages synced from the desktop stand-in")
        XCTAssertTrue(app.staticTexts["Reading list"].exists)
        shared.tap()
        XCTAssertTrue(app.textFields.matching(NSPredicate(format: "value == 'Groceries'")).firstMatch.waitForExistence(timeout: 20)
                      || app.textViews.matching(NSPredicate(format: "value == 'Groceries'")).firstMatch.waitForExistence(timeout: 5))

        // Edit on the phone: add a block and type into it.
        app.buttons["add-block"].firstMatch.tap()
        let typed = "typed on the iPhone"
        app.typeText(typed)
        sleep(4)

        // Create a page on the phone too.
        app.navigationBars.buttons.firstMatch.tap()
        sleep(1)
        print("PHONE_DONE")
    }
}
