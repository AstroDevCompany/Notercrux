import SwiftUI
import UIKit

struct SyncView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Group {
                if model.status.isLinked { LinkedView() } else { SetupView() }
            }
            .navigationTitle("Sync")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .confirmationAction) { Button("Done") { dismiss() } } }
        }
        .onAppear { model.refreshStatus() }
    }
}

private struct SetupView: View {
    @Environment(AppModel.self) private var model
    @State private var tab = 0
    @State private var code = DebugLaunch.invite ?? ""
    @State private var server = ""
    @State private var workspace = ""
    @State private var phrase = ""
    @State private var label = UIDevice.current.name
    @State private var keepLocal = true
    @State private var sas: String?
    @State private var busy = false
    @State private var error: String?

    var body: some View {
        Form {
            if let sas {
                Section("Approve on your other device") {
                    Text("Open Notercrux on the device that already syncs and approve this request. It must show the same code:")
                    Text(sas).font(.system(size: 40, weight: .semibold, design: .rounded)).tracking(8)
                        .frame(maxWidth: .infinity).accessibilityIdentifier("sas")
                    HStack { ProgressView(); Text("Waiting for approval…") }
                    Button("Cancel", role: .cancel) { try? model.core.cancelJoin(); self.sas = nil }
                }
            } else {
                Picker("", selection: $tab) {
                    Text("Join with a code").tag(0)
                    Text("Recover").tag(1)
                }.pickerStyle(.segmented).listRowBackground(Color.clear)

                if model.status.phase == "keysMissing" {
                    Text("This device's encryption keys are missing. Recover the workspace with your recovery key.")
                        .foregroundStyle(.red)
                }
                Section {
                    if tab == 0 {
                        TextField("Invite code (NCX1-…)", text: $code, axis: .vertical)
                            .textInputAutocapitalization(.never).autocorrectionDisabled()
                            .font(.system(.footnote, design: .monospaced)).accessibilityIdentifier("invite-code")
                        Button("Paste from clipboard") { code = UIPasteboard.general.string ?? code }
                    } else {
                        TextField("Server (https://…)", text: $server).textInputAutocapitalization(.never).autocorrectionDisabled().keyboardType(.URL)
                        TextField("Workspace ID", text: $workspace).textInputAutocapitalization(.never).autocorrectionDisabled()
                        TextField("Recovery key", text: $phrase).textInputAutocapitalization(.characters).autocorrectionDisabled()
                    }
                    TextField("Name of this device", text: $label)
                } footer: {
                    Text(tab == 0
                         ? "On a device that already syncs: Settings, Sync, Add device. Enter the code it shows."
                         : "Lost every device? Your recovery key restores your notes here.")
                }
                Section("Pages already on this device") {
                    Picker("", selection: $keepLocal) {
                        Text("Keep them and add to the workspace").tag(true)
                        Text("Replace them with the workspace").tag(false)
                    }.pickerStyle(.inline).labelsHidden()
                }
                if let error { Text(error).foregroundStyle(.red).accessibilityIdentifier("error") }
                Section {
                    Button(tab == 0 ? "Continue" : "Recover") { Task { await submit() } }
                        .disabled(busy || label.isEmpty || (tab == 0 ? code.trimmingCharacters(in: .whitespaces).isEmpty : server.isEmpty || workspace.isEmpty || phrase.isEmpty))
                        .accessibilityIdentifier("continue")
                }
            }
        }
        .task(id: sas) { await pollLoop() }
        .onAppear { if model.core.joinInProgress() && sas == nil { sas = "" } }
        .task {
            // Debug builds only: a code passed at launch is submitted straight away (testing on a
            // real device without typing 240 characters).
            if DebugLaunch.invite != nil, sas == nil, !busy, !model.status.isLinked { await submit() }
        }
    }

    private func submit() async {
        busy = true; error = nil
        defer { busy = false }
        do {
            if tab == 0 {
                sas = try await model.core.beginJoin(code: code, label: label)
            } else {
                try await model.core.recover(server: server, workspaceId: workspace.trimmingCharacters(in: .whitespaces), phrase: phrase, label: label, mode: keepLocal ? "keepLocal" : "replaceLocal")
                model.afterLinkChange()
            }
        } catch {
            self.error = Self.message(error)
            #if DEBUG
            print("NCX join failed: \(Self.message(error))")
            #endif
        }
    }

    private func pollLoop() async {
        guard sas != nil else { return }
        while !Task.isCancelled {
            try? await Task.sleep(for: .seconds(2))
            if Task.isCancelled { return }
            do {
                if try await model.core.pollJoin(mode: keepLocal ? "keepLocal" : "replaceLocal") == "joined" {
                    sas = nil
                    model.afterLinkChange()
                    return
                }
            } catch { self.error = Self.message(error); sas = nil; return }
        }
    }

    static func message(_ e: Error) -> String {
        if case CoreError.Message(let m) = e { return m }
        return "\(e)"
    }
}

private struct LinkedView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss
    @State private var devices: [DeviceView] = []
    @State private var confirmLeave = false
    @State private var removing: DeviceView?
    @State private var error: String?
    @State private var showLog = false

    var body: some View {
        let s = model.status
        List {
            Section {
                LabeledContent("Status", value: statusText(s))
                if let server = s.server { LabeledContent("Server", value: server) }
                LabeledContent("Last sync", value: s.lastSyncAt.map { Date(timeIntervalSince1970: TimeInterval($0)).formatted(.relative(presentation: .named)) } ?? "never")
                if s.pendingOps > 0 { LabeledContent("Waiting to upload", value: "\(s.pendingOps) changes") }
                if s.attachmentsPending > 0 { LabeledContent("Files in transfer", value: "\(s.attachmentsPending)") }
                if let e = s.lastError, s.phase != "idle" { Text(e).font(.footnote).foregroundStyle(.red) }
                if s.phase == "revoked" {
                    Text("This device was removed from the workspace. Your pages are still here, including changes that were never uploaded.")
                        .font(.footnote)
                    Button("Keep my pages and disconnect") { Task { try? await model.core.leaveLocal(); model.afterLinkChange() } }
                }
                Button("Sync now") { model.syncNow() }
            }
            if s.phase != "revoked" {
                Section("Devices") {
                    ForEach(devices.filter { !$0.revoked }) { d in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(d.label + (d.current ? " (this device)" : ""))
                                if let seen = d.lastSeen { Text("Active " + Date(timeIntervalSince1970: TimeInterval(seen)).formatted(.relative(presentation: .named))).font(.caption).foregroundStyle(.secondary) }
                            }
                            Spacer()
                            if !d.current { Button("Remove", role: .destructive) { removing = d }.buttonStyle(.borderless) }
                        }
                    }
                    if let error { Text(error).foregroundStyle(.red).font(.footnote) }
                }
            }
            Section {
                Button("Show activity") { showLog.toggle() }
                if showLog { ForEach(model.events.reversed()) { e in Text(e.message).font(.caption).foregroundStyle(e.level == "error" ? .red : .primary) } }
            }
            Section {
                Button("Disconnect this device", role: .destructive) { confirmLeave = true }
            } footer: { Text("Pages stay on this device.") }
        }
        .task(id: s.epoch) { await loadDevices() }
        .confirmationDialog("Disconnect this device?", isPresented: $confirmLeave, titleVisibility: .visible) {
            Button("Disconnect", role: .destructive) { Task { try? await model.core.leave(); model.afterLinkChange(); dismiss() } }
        } message: { Text("It stops syncing and keeps its pages. If other devices remain, this one is removed and the key is replaced.") }
        .confirmationDialog("Remove \(removing?.label ?? "device")?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove", role: .destructive) {
                if let d = removing { Task { do { try await model.core.revokeDevice(id: d.id); await loadDevices() } catch { self.error = SetupView.message(error) } } }
            }
        } message: { Text("It stops syncing and cannot read anything written from now on. Your other devices get a new encryption key.") }
    }

    private func loadDevices() async {
        guard model.status.phase != "revoked" else { return }
        do { devices = try Coding.decode([DeviceView].self, try await model.core.devicesJson()); error = nil }
        catch { self.error = SetupView.message(error) }
    }

    private func statusText(_ s: SyncStatus) -> String {
        ["idle": "Up to date", "syncing": "Syncing…", "offline": "Offline, will retry", "error": "Problem",
         "revoked": "Removed from workspace"][s.phase] ?? s.phase
    }
}
