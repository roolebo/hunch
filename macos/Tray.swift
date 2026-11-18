// The tray of hunch on macOS: it runs hunch-host as the human's service,
// shows what the seat does, and sets the seat from its settings window.

import AVFoundation
import AppKit
import ApplicationServices
import ServiceManagement
import SwiftUI

/// What went wrong, as the human reads it, with hunch-host's exit status
struct Failure: Error {
    let message: String
    var status: Int32 = 1
}

/// pick's exit status when the human pressed Escape
let cancelled: Int32 = 5

let hostURL = Bundle.main.bundleURL.appendingPathComponent("Contents/MacOS/hunch-host")

/// Runs the hunch-host next to this app, or HOST's over ssh
func hunchHost(_ arguments: [String], on host: String = "localhost") -> Result<Data, Failure> {
    let process = Process()
    if host == "localhost" {
        process.executableURL = hostURL
        process.arguments = arguments
    } else {
        process.executableURL = URL(fileURLWithPath: "/usr/bin/ssh")
        process.arguments = ["-o", "BatchMode=yes", host, "hunch-host"] + arguments
    }
    let out = Pipe()
    let err = Pipe()
    process.standardOutput = out
    process.standardError = err
    do {
        try process.run()
    } catch {
        return .failure(Failure(message: "can't run hunch-host: \(error.localizedDescription)"))
    }
    let data = out.fileHandleForReading.readDataToEndOfFile()
    process.waitUntilExit()
    guard process.terminationStatus == 0 else {
        // The first line: the rest is usage, too long for a menu or the pill
        let said = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .split(separator: "\n", maxSplits: 1).first.map(String.init)?
            .replacingOccurrences(of: "hunch-host: ", with: "") ?? ""
        return .failure(Failure(message: said, status: process.terminationStatus))
    }
    return .success(data)
}

func decode<T: Decodable>(_ type: T.Type, _ arguments: [String], on host: String = "localhost") -> T? {
    guard case .success(let data) = hunchHost(arguments, on: host) else { return nil }
    return try? JSONDecoder().decode(type, from: data)
}

struct Need: Decodable {
    let `for`: String
    let grant: String
    /// Set when needs lists every grant
    let granted: Bool?
}

/// What hunch-host needs, or with all every grant it uses; ask has macOS
/// ask the human in hunch-host's name
func needs(ask: Bool = false, all: Bool = false) -> Result<[Need], Failure> {
    hunchHost(["needs"] + (ask ? ["--ask"] : []) + (all ? ["--all"] : [])).flatMap { data in
        Result { try JSONDecoder().decode([Need].self, from: data) }
            .mapError { Failure(message: "hunch-host's needs aren't JSON: \($0.localizedDescription)") }
    }
}

/// Where System Settings lets the human grant each thing, and its icon
let panes = [
    "Screen Recording": ("Privacy_ScreenCapture", "rectangle.dashed.badge.record"),
    "Accessibility": ("Privacy_Accessibility", "accessibility"),
    "Microphone": ("Privacy_Microphone", "mic"),
    "Camera": ("Privacy_Camera", "camera"),
]

/// macOS ties hunch-host's grants to this app, and lists an app in System
/// Settings only once the app itself asks; the service asks too, for a
/// grant that would land on it. macOS prompts once; after that, only
/// System Settings grants.
func ask(_ grant: String) {
    // The microphone prompt is the grant itself, no System Settings needed
    if grant == "Microphone", AVCaptureDevice.authorizationStatus(for: .audio) == .notDetermined {
        AVCaptureDevice.requestAccess(for: .audio) { _ in }
        return
    }
    switch grant {
    case "Screen Recording":
        _ = CGRequestScreenCaptureAccess()
    case "Accessibility":
        let prompt = kAXTrustedCheckOptionPrompt.takeUnretainedValue() as String
        _ = AXIsProcessTrustedWithOptions([prompt: true] as CFDictionary)
    default:
        break
    }
    _ = needs(ask: true)
    let pane = panes[grant].map { "?\($0.0)" } ?? ""
    NSWorkspace.shared.open(URL(string: "x-apple.systempreferences:com.apple.preference.security\(pane)")!)
}

/// The seat as watch prints it
struct SeatState: Decodable {
    let recording: Bool
    let opening: Bool
    let transcribing: Bool
    let loading: Bool
    /// COMPUTE, as HOST ADDR; nil until the human picks one, on a host
    /// without a compute
    let compute: String?
    let model: String?
    let target: String
    let submit: Bool
    let key: String
    let preload: Bool
    /// Minutes the model stays in GPU memory while idle; nil is always
    let warm: Int?
    let error: String?
}

/// COMPUTE's host and ADDR
func split(_ compute: String) -> (host: String, addr: String) {
    let parts = compute.split(separator: " ", maxSplits: 1).map(String.init)
    return (parts[0], parts[1])
}

/// A device as the seat's devices prints it
struct Device: Decodable {
    let id: String
    let kind: String
    let name: String
    let inUse: Bool

    enum CodingKeys: String, CodingKey {
        case id, kind, name
        case inUse = "in_use"
    }
}

/// The browser role as status prints it
struct BrowserStatus: Decodable {
    let browsers: [String]
    let serves: String
    let allow: [String]
}

/// The seat's config, which only the human writes, here through the app;
/// SEAT.md specifies it
enum SeatConfig {
    static var url: URL {
        let config = ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".config")
        return config.appendingPathComponent("hunch/seat.json")
    }

    /// The config, empty if there's none; it throws rather than let a
    /// write replace a file it can't read
    static func read() throws -> [String: Any] {
        guard FileManager.default.fileExists(atPath: url.path) else { return [:] }
        guard let config = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [String: Any] else {
            throw Failure(message: "\(url.path) isn't a JSON object")
        }
        return config
    }

    /// The allow of what's at PATH: ["browser"], or ["devices", DEVICE]
    static func allow(_ path: [String]) throws -> [String] {
        var node: Any? = try read()
        for key in path {
            node = (node as? [String: Any])?[key]
        }
        return (node as? [String: Any])?["allow"] as? [String] ?? []
    }

    static func setAllow(_ path: [String], _ hosts: [String]) throws {
        let config = setting(try read(), path + ["allow"], to: hosts)
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try JSONSerialization.data(withJSONObject: config, options: [.prettyPrinted, .sortedKeys])
            .write(to: url, options: .atomic)
    }

    /// OBJECT with VALUE at PATH, and the objects on the way where missing
    static func setting(_ object: [String: Any], _ path: [String], to value: Any) -> [String: Any] {
        guard let key = path.first else { return object }
        var object = object
        object[key] = path.count == 1 ? value
            : setting(object[key] as? [String: Any] ?? [:], Array(path.dropFirst()), to: value)
        return object
    }
}

/// A HOST as the human calls it
func hostName(_ host: String) -> String {
    host == "localhost" ? "This Mac" : host
}

struct Model: Decodable, Identifiable {
    let id: String
    let name: String?
    let size: Int64
    let loaded: Bool
}

struct WindowInfo: Decodable {
    let id: String
    let app: String
    let title: String
}

let showInDockKey = "showInDock"

/// Puts the app in the Dock or takes it out; the tray icon stays
@MainActor
func showInDock(_ shown: Bool) {
    NSApp.setActivationPolicy(shown ? .regular : .accessory)
}

/// What the settings window shows
@MainActor
final class Settings: ObservableObject {
    struct Grant: Identifiable {
        let need: Need
        var allowed: Bool
        var id: String { need.grant }
    }

    @Published var grants: [Grant] = []
    @Published var error: String?
    /// The models of the seat's compute
    @Published var models: [Model] = []
    @Published var seat: SeatState?
    /// The roles the service serves
    @Published var roles: [String] = []
    /// The microphone in use
    @Published var mic: Device?
    /// The hosts its speech may go to
    @Published var micAllow: [String] = []
    /// The hosts the browser's pages may go to
    @Published var browserAllow: [String] = []
    @Published var inDock = UserDefaults.standard.bool(forKey: showInDockKey)

    var missing: Bool { grants.contains { !$0.allowed } }

    func refresh() {
        switch needs(all: true) {
        case .success(let all):
            error = nil
            grants = all.map { Grant(need: $0, allowed: $0.granted ?? false) }
        case .failure(let failure):
            error = failure.message
        }
        refreshHosts()
    }

    /// What the menu needs besides the seat: where data may go
    func refreshHosts() {
        roles = decode([String].self, ["roles"]) ?? []
        mic = decode([Device].self, ["seat", "devices"])?.first { $0.kind == "mic" && $0.inUse }
        do {
            micAllow = try mic.map { try SeatConfig.allow(["devices", $0.id]) } ?? []
            browserAllow = try SeatConfig.allow(["browser"])
        } catch {
            self.error = error.localizedDescription
        }
        refreshModels()
    }

    /// Asks for the models in the background, since another host's come
    /// over ssh
    func refreshModels() {
        guard let compute = seat?.compute else {
            models = []
            return
        }
        let (host, addr) = split(compute)
        DispatchQueue.global().async {
            let models = decode([Model].self, ["compute", addr, "models"], on: host) ?? []
            DispatchQueue.main.async {
                if self.seat?.compute == compute {
                    self.models = models
                }
            }
        }
    }

    func setAllow(_ path: [String], _ hosts: [String]) {
        do {
            try SeatConfig.setAllow(path, hosts)
        } catch {
            self.error = error.localizedDescription
        }
        refreshHosts()
    }

    func seatChanged(_ state: SeatState) {
        let changed = seat?.compute != state.compute || seat?.loading != state.loading
        seat = state
        if changed {
            refreshModels()
        }
    }
}

/// A TARGET as the human knows the window: its app and title
func describe(_ target: String) -> String {
    if target == "focus" {
        return "Focused window"
    }
    let id = target.split(separator: " ").last.map(String.init)
    guard let window = (decode([WindowInfo].self, ["wm", "macos:", "windows"]) ?? []).first(where: { $0.id == id })
    else { return target }
    return window.title.isEmpty ? window.app : "\(window.app) — \(window.title)"
}

struct GrantRows: View {
    @ObservedObject var settings: Settings

    var body: some View {
        ForEach(settings.grants) { grant in
            LabeledContent {
                if grant.allowed {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                } else {
                    Button("Allow") { ask(grant.id) }
                }
            } label: {
                Label(grant.id, systemImage: panes[grant.id]?.1 ?? "lock")
                Text("For \(grant.need.for)")
            }
        }
    }
}

/// The first thing a human sees: what to grant, gone once all is granted
struct PermissionsView: View {
    @ObservedObject var settings: Settings

    var body: some View {
        Form {
            SwiftUI.Section {
                KeyRow(settings: settings)
            } header: {
                Text("Dictation")
            } footer: {
                Text("Hold the key and speak; Escape cancels.").foregroundStyle(.secondary)
            }
            SwiftUI.Section {
                GrantRows(settings: settings)
            } header: {
                Text("Hunch needs a few permissions")
            } footer: {
                Text("macOS asks before any app can see your windows, type, or hear you or your keys. Nothing leaves this Mac.")
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .scrollIndicators(.never)
        .frame(width: 440)
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// Restarts the service the app runs, for a setting it reads at start
@MainActor
func restartService() {
    (NSApp.delegate as? Tray)?.service?.terminate()
}

/// The dictation keys hunch-host knows, by its names, as the human reads
/// them, and by the keycodes macOS gives them
let dictationKeys: [(name: String, label: String, code: UInt16)] = [
    ("right-super", "Right ⌘", 54),
    ("right-alt", "Right ⌥", 61),
    ("right-control", "Right ⌃", 62),
    ("right-shift", "Right ⇧", 60),
    ("fn", "fn", 63),
]

/// The dictation key; Change… takes the next one the human presses, since
/// a keyboard's labels may not be the keys macOS sees, as a PC keyboard's
/// right Alt is the right Command key
struct KeyRow: View {
    @ObservedObject var settings: Settings
    @State var monitor: Any?

    var body: some View {
        LabeledContent("Dictate while holding") {
            HStack {
                if monitor == nil {
                    Text(dictationKeys.first { $0.name == settings.seat?.key }?.label ?? "None")
                    Button("Change…", action: listen)
                } else {
                    Text("Press a key…").foregroundStyle(.secondary)
                    Button("Cancel", action: stop)
                }
            }
        }
    }

    // Keys pressed in the app's own window need no grant
    func listen() {
        monitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) { event in
            if let key = dictationKeys.first(where: { $0.code == event.keyCode }) {
                _ = hunchHost(["seat", "key", key.name])
                stop()
            }
            return event
        }
    }

    func stop() {
        monitor.map(NSEvent.removeMonitor)
        monitor = nil
    }
}

/// The hosts in an allow, which the human edits here
struct AllowSection: View {
    let title: String
    let footer: String
    let hosts: [String]
    let set: ([String]) -> Void
    /// The host the human types in to allow
    @State var host = ""

    var body: some View {
        SwiftUI.Section {
            ForEach(hosts, id: \.self) { allowed in
                LabeledContent(allowed) {
                    Button("Remove") { set(hosts.filter { $0 != allowed }) }
                }
            }
            HStack {
                TextField("Host", text: $host, prompt: Text("as you ssh to it"))
                    .onSubmit(allow)
                Button("Allow", action: allow).disabled(!allowable)
            }
        } header: {
            Text(title)
        } footer: {
            Text(footer).foregroundStyle(.secondary)
        }
    }

    /// ssh would take a host that starts with - for an option
    var allowable: Bool {
        let typed = host.trimmingCharacters(in: .whitespaces)
        return !typed.isEmpty && !typed.hasPrefix("-") && !typed.contains(" ")
            && typed != "localhost" && !hosts.contains(typed)
    }

    func allow() {
        guard allowable else { return }
        set(hosts + [host.trimmingCharacters(in: .whitespaces)])
        host = ""
    }
}

struct SettingsView: View {
    enum Tab: CaseIterable {
        case general, permissions, dictation, browser, hotkeys, about
    }

    @ObservedObject var settings: Settings
    let tab: Tab

    var body: some View {
        switch tab {
        case .general: general
        case .permissions: permissions
        case .dictation: dictation
        case .browser: browser
        case .hotkeys: hotkeys
        case .about: about
        }
    }

    /// Sets the seat through hunch-host; the change comes back by watch
    nonisolated func seat(_ arguments: String...) -> @Sendable (String) -> Void {
        { value in _ = hunchHost(["seat"] + arguments + [value]) }
    }

    var general: some View {
        page {
            Toggle(
                "Show in Dock",
                isOn: Binding(
                    get: { settings.inDock },
                    set: {
                        settings.inDock = $0
                        UserDefaults.standard.set($0, forKey: showInDockKey)
                        showInDock($0)
                    }
                )
            )
        }
    }

    var permissions: some View {
        page { GrantRows(settings: settings) }
    }

    var dictation: some View {
        page {
            SwiftUI.Section {
                Picker("Model", selection: Binding(get: { settings.seat?.model ?? "" }, set: seat("model"))) {
                    if settings.seat?.model == nil {
                        Text("None").tag("")
                    }
                    ForEach(settings.models) { model in
                        Text(
                            [model.name ?? model.id, ByteCountFormatter.string(fromByteCount: model.size, countStyle: .file)]
                                .joined(separator: " · ") + (model.loaded ? " · loaded" : "")
                        )
                        .tag(model.id)
                    }
                }
                Toggle(
                    "Load the model at start",
                    isOn: Binding(
                        get: { settings.seat?.preload ?? true },
                        set: { seat("preload")($0 ? "on" : "off") }
                    )
                )
                .help("Off loads it at the first dictation, and frees its memory now")
                Picker(
                    "Keep the model ready",
                    selection: Binding(
                        get: { settings.seat?.warm.map(String.init) ?? "always" },
                        set: {
                            seat("warm")($0)
                            restartService()
                        }
                    )
                ) {
                    Text("3 minutes").tag("3")
                    Text("10 minutes").tag("10")
                    Text("1 hour").tag("60")
                    Text("Always").tag("always")
                }
                .help("How long the model stays in GPU memory while you don't dictate; after that, the next dictation waits a moment")
                Toggle(
                    "Press Enter after the text",
                    isOn: Binding(
                        get: { settings.seat?.submit ?? true },
                        set: { seat("submit")($0 ? "on" : "off") }
                    )
                )
            } footer: {
                Text(speechGoes).foregroundStyle(.secondary)
            }
            if let mic = settings.mic {
                AllowSection(
                    title: "Speech may go to",
                    footer: "Of \(mic.name), to transcribe it there, once you pick the host in the menu; Hunch reaches it by ssh, as you do.",
                    hosts: settings.micAllow
                ) { settings.setAllow(["devices", mic.id], $0) }
            }
        }
    }

    var speechGoes: String {
        switch settings.seat?.compute.map(split)?.host {
        case "localhost": "Speech turns into text on this Mac; nothing leaves it."
        case let host?: "Speech goes to \(host), which turns it into text."
        case nil: "Pick where speech goes in the menu."
        }
    }

    var browser: some View {
        page {
            AllowSection(
                title: "Pages may go to",
                footer: "Pages go to one host at a time, the one you pick in the menu; Hunch reaches it by ssh, as you do.",
                hosts: settings.browserAllow
            ) { settings.setAllow(["browser"], $0) }
        }
    }

    var hotkeys: some View {
        page {
            KeyRow(settings: settings)
            LabeledContent("Cancel", value: "Esc")
        }
    }

    var about: some View {
        page {
            SwiftUI.Section {
                HStack(spacing: 12) {
                    Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 48, height: 48)
                    VStack(alignment: .leading) {
                        Text("Hunch").font(.headline)
                        Text("Version \(Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? "")")
                            .foregroundStyle(.secondary)
                    }
                }
            }
            SwiftUI.Section("Ideas") {
                credit("Vannevar Bush", "As We May Think, 1945", "https://www.theatlantic.com/magazine/archive/1945/07/as-we-may-think/303881/")
                credit("J. C. R. Licklider", "Man-Computer Symbiosis, 1960", "https://groups.csail.mit.edu/medg/people/psz/Licklider.html")
                credit("Douglas Engelbart", "Augmenting Human Intellect, 1962", "https://www.dougengelbart.org/")
            }
            SwiftUI.Section("Built on") {
                credit("ggml", "Georgi Gerganov and its authors", "https://github.com/ggml-org/ggml")
                credit("llama.cpp", "Its authors", "https://github.com/ggml-org/llama.cpp")
                credit("transcribe.cpp", "Its authors", "https://github.com/handy-computer/transcribe.cpp")
            }
            SwiftUI.Section("Inspiration") {
                credit("Pipit", "The first dictation app that got Hunch going", "https://www.pipitvoice.com")
            }
        }
    }

    func credit(_ name: String, _ work: String, _ url: String) -> some View {
        LabeledContent(name) { Link(work, destination: URL(string: url)!) }
    }

    /// The tabs, as macOS lays settings out: in the window's toolbar
    @MainActor
    static func tabs(_ settings: Settings) -> NSTabViewController {
        let tabs = NSTabViewController()
        tabs.tabStyle = .toolbar
        // The window says Settings, whichever tab is open
        tabs.canPropagateSelectedChildViewControllerTitle = false
        let pages: [(String, String, Tab)] = [
            ("General", "gearshape", .general),
            ("Permissions", "lock.shield", .permissions),
            ("Dictation", "waveform", .dictation),
            ("Browser", "globe", .browser),
            ("Hotkeys", "keyboard", .hotkeys),
            ("About", "info.circle", .about),
        ]
        for (title, symbol, tab) in pages {
            let host = NSHostingController(rootView: SettingsView(settings: settings, tab: tab))
            // The window takes each tab's own height
            host.sizingOptions = .preferredContentSize
            let item = NSTabViewItem(viewController: host)
            item.label = title
            item.image = NSImage(systemSymbolName: symbol, accessibilityDescription: title)
            tabs.addTabViewItem(item)
        }
        return tabs
    }

    /// A tab: a grouped form, with hunch-host's error on top if it has one
    func page(@ViewBuilder _ content: () -> some View) -> some View {
        Form {
            if let error = settings.error {
                Label(error, systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                    .textSelection(.enabled)
            }
            content()
        }
        .formStyle(.grouped)
        // The window takes the form's height, so there's nothing to scroll
        .scrollIndicators(.never)
        .frame(width: 460)
        .fixedSize(horizontal: false, vertical: true)
    }
}

/// The lines of a pipe, until it closes
struct LineReader: Sequence, IteratorProtocol {
    let handle: FileHandle
    var pending = Data()

    init(_ handle: FileHandle) {
        self.handle = handle
    }

    mutating func next() -> Data? {
        while true {
            if let newline = pending.firstIndex(of: UInt8(ascii: "\n")) {
                let line = pending[..<newline]
                pending = Data(pending[pending.index(after: newline)...])
                return Data(line)
            }
            let chunk = handle.availableData
            if chunk.isEmpty {
                return nil
            }
            pending.append(chunk)
        }
    }
}

/// A panel above every window that never takes the mouse
func floatingPanel() -> NSPanel {
    let panel = NSPanel(
        contentRect: .zero,
        styleMask: [.borderless, .nonactivatingPanel],
        backing: .buffered,
        defer: true
    )
    panel.level = .statusBar
    panel.backgroundColor = .clear
    panel.isOpaque = false
    panel.ignoresMouseEvents = true
    panel.collectionBehavior = [.canJoinAllSpaces, .fullScreenAuxiliary, .stationary]
    return panel
}

/// What the overlay shows, at the bottom of the screen
@MainActor
final class Overlay: ObservableObject {
    enum Mode: Equatable {
        case hidden, opening, listening, loading, transcribing, picking
        case error(String)
    }

    @Published var mode = Mode.hidden
    let panel = floatingPanel()

    init() {
        panel.hasShadow = true
        panel.contentView = NSHostingView(rootView: Pill(overlay: self))
    }

    func show(_ mode: Mode) {
        self.mode = mode
        guard mode != .hidden, let screen = NSScreen.main, let view = panel.contentView else {
            panel.orderOut(nil)
            return
        }
        let size = view.fittingSize
        let frame = screen.visibleFrame
        panel.setFrame(
            NSRect(x: frame.midX - size.width / 2, y: frame.minY + 48, width: size.width, height: size.height),
            display: true
        )
        panel.orderFrontRegardless()
    }
}

struct Pill: View {
    @ObservedObject var overlay: Overlay
    @State var pulse = false

    var body: some View {
        HStack(spacing: 10) {
            switch overlay.mode {
            case .listening:
                Circle()
                    .fill(.red)
                    .frame(width: 10, height: 10)
                    .opacity(pulse ? 0.35 : 1)
                    .animation(.easeInOut(duration: 0.7).repeatForever(), value: pulse)
                    .onAppear { pulse = true }
                    .onDisappear { pulse = false }
                Text("Listening…")
            case .opening:
                ProgressView().controlSize(.small)
                Text("Opening the microphone…")
            case .loading:
                ProgressView().controlSize(.small)
                Text("Loading model…")
            case .transcribing:
                ProgressView().controlSize(.small)
                Text("Transcribing…")
            case .picking:
                Image(systemName: "cursorarrow.click.2")
                Text("Click a window · Esc cancels")
            case .error(let message):
                Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow)
                Text(message).lineLimit(2).frame(maxWidth: 380, alignment: .leading)
            case .hidden:
                EmptyView()
            }
        }
        .font(.system(size: 13, weight: .medium))
        .foregroundStyle(.white)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .background(.black.opacity(0.8), in: Capsule())
        .fixedSize()
        .padding(8)
    }
}

/// Outlines the window under the pointer while the human picks one. pick
/// in hunch-host takes the window under the click by the same rule: the
/// front window on the normal layer.
@MainActor
final class Outline {
    let panel = floatingPanel()
    var timer: Timer?

    init() {
        let border = NSView()
        border.wantsLayer = true
        border.layer?.borderColor = NSColor.controlAccentColor.cgColor
        border.layer?.borderWidth = 4
        border.layer?.cornerRadius = 10
        panel.contentView = border
    }

    func start() {
        timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) { _ in
            MainActor.assumeIsolated { self.follow() }
        }
    }

    func stop() {
        timer?.invalidate()
        timer = nil
        panel.orderOut(nil)
    }

    func follow() {
        guard let frame = windowUnderPointer() else {
            panel.orderOut(nil)
            return
        }
        panel.setFrame(frame, display: true)
        panel.orderFrontRegardless()
    }

    /// In AppKit's coordinates: from the bottom left of the main display,
    /// where CoreGraphics counts from its top left
    func windowUnderPointer() -> NSRect? {
        guard let main = NSScreen.screens.first,
            let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
                as? [[String: Any]]
        else { return nil }
        let pointer = NSEvent.mouseLocation
        let flipped = CGPoint(x: pointer.x, y: main.frame.height - pointer.y)
        let own = ProcessInfo.processInfo.processIdentifier
        for info in list {
            guard info[kCGWindowLayer as String] as? Int == 0,
                (info[kCGWindowAlpha as String] as? Double ?? 1) > 0,
                info[kCGWindowOwnerPID as String] as? Int32 != own,
                let bounds = info[kCGWindowBounds as String] as? NSDictionary,
                let rect = CGRect(dictionaryRepresentation: bounds),
                rect.contains(flipped)
            else { continue }
            return NSRect(x: rect.minX, y: main.frame.height - rect.maxY, width: rect.width, height: rect.height)
        }
        return nil
    }
}

@MainActor
final class Tray: NSObject, NSApplicationDelegate, NSMenuDelegate, NSWindowDelegate {
    let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
    var service: Process?
    var quitting = false
    let overlay = Overlay()
    let outline = Outline()
    let settings = Settings()
    var registerError: String?
    var settingsWindow: NSWindow?
    var permissionsWindow: NSWindow?
    var timer: Timer?
    var picking = false
    var errorShown: String?

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showSettings()
        return false
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        if settings.inDock {
            showInDock(true)
        }
        item.button?.image = trayIcon()
        NSApp.mainMenu = mainMenu()
        let menu = NSMenu()
        menu.delegate = self
        item.menu = menu
        // The app starts at login and runs the service itself: macOS runs a
        // launchd agent only as the binary it registered, which an ad hoc
        // signed build isn't for long
        do {
            try SMAppService.mainApp.register()
        } catch {
            registerError = error.localizedDescription
        }
        serve()
        watchSeat()
        if case .success(let now) = needs(), now.isEmpty {
            return
        }
        showPermissions()
    }

    /// Runs hunch-host serve for as long as the app runs, again if it
    /// ends; a service already running, e.g. under launchd, makes it end
    func serve() {
        let process = Process()
        process.executableURL = hostURL
        process.arguments = ["serve"]
        let logs = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Logs/Hunch")
        try? FileManager.default.createDirectory(at: logs, withIntermediateDirectories: true)
        let log = logs.appendingPathComponent("hunch-host.log")
        if !FileManager.default.fileExists(atPath: log.path) {
            FileManager.default.createFile(atPath: log.path, contents: nil)
        }
        let handle = try? FileHandle(forWritingTo: log)
        handle?.seekToEndOfFile()
        process.standardError = handle ?? FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        process.terminationHandler = { _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                if !self.quitting {
                    self.serve()
                }
            }
        }
        do {
            try process.run()
            service = process
        } catch {
            registerError = "can't run hunch-host: \(error.localizedDescription)"
        }
    }

    func applicationWillTerminate(_ notification: Notification) {
        quitting = true
        service?.terminate()
    }

    /// Follows seat watch for as long as the app runs, again after the
    /// service restarts
    func watchSeat() {
        let process = Process()
        process.executableURL = hostURL
        process.arguments = ["seat", "watch"]
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        // A thread of its own reads the lines, so none waits on the main one
        Thread {
            for line in LineReader(out.fileHandleForReading) {
                if let state = try? JSONDecoder().decode(SeatState.self, from: line) {
                    DispatchQueue.main.async { self.seatChanged(state) }
                }
            }
        }.start()
        process.terminationHandler = { _ in
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { self.watchSeat() }
        }
        try? process.run()
    }

    func seatChanged(_ state: SeatState) {
        // The icon stays: macOS's own sign shows the microphone records
        settings.seatChanged(state)
        if picking {
            return
        }
        if state.opening {
            overlay.show(.opening)
        } else if state.recording {
            overlay.show(.listening)
        } else if state.transcribing {
            overlay.show(state.loading ? .loading : .transcribing)
        } else if let error = state.error, error != errorShown {
            showError(error)
        } else if case .error = overlay.mode {
            // An error stays until its time is up
        } else {
            overlay.show(.hidden)
        }
        errorShown = state.error
    }

    func showError(_ message: String) {
        overlay.show(.error(message))
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            if case .error(message) = self.overlay.mode {
                self.overlay.show(.hidden)
            }
        }
    }

    /// Lets the human click the window dictation goes to
    func pickWindow() {
        picking = true
        overlay.show(.picking)
        outline.start()
        DispatchQueue.global().async {
            let picked = hunchHost(["wm", "macos:", "pick"]).flatMap { data in
                let window = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                return hunchHost(["seat", "target", "localhost", "macos:", window])
            }
            DispatchQueue.main.async {
                self.picking = false
                self.outline.stop()
                self.overlay.show(.hidden)
                if case .failure(let failure) = picked, failure.status != cancelled {
                    self.showError(failure.message)
                }
            }
        }
    }

    /// A window of the app's, made once and shown in front
    func show(_ title: String, _ existing: NSWindow?, _ content: () -> NSViewController) -> NSWindow {
        if existing == nil {
            settings.refresh()
            // Grants happen in System Settings, so the windows keep looking
            if timer == nil {
                timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
                    MainActor.assumeIsolated { self.tick() }
                }
            }
        }
        let window = existing ?? {
            let window = NSWindow(contentViewController: content())
            window.styleMask = [.titled, .closable]
            window.toolbarStyle = .preference
            window.title = title
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.center()
            return window
        }()
        NSApp.activate(ignoringOtherApps: true)
        window.makeKeyAndOrderFront(nil)
        return window
    }

    func showSettings(_ tab: SettingsView.Tab? = nil) {
        settingsWindow = show("Settings", settingsWindow) { SettingsView.tabs(settings) }
        if let tab, let tabs = settingsWindow?.contentViewController as? NSTabViewController {
            tabs.selectedTabViewItemIndex = SettingsView.Tab.allCases.firstIndex(of: tab) ?? 0
        }
    }

    func showPermissions() {
        permissionsWindow = show("Hunch", permissionsWindow) { NSHostingController(rootView: PermissionsView(settings: settings)) }
    }

    func tick() {
        settings.refresh()
        // The permissions dialog has done its job once all is granted
        if !settings.missing, let window = permissionsWindow {
            window.close()
        }
    }

    func windowWillClose(_ notification: Notification) {
        let closing = notification.object as? NSWindow
        if closing === settingsWindow {
            settingsWindow = nil
        }
        if closing === permissionsWindow {
            permissionsWindow = nil
        }
        if settingsWindow == nil && permissionsWindow == nil {
            timer?.invalidate()
            timer = nil
        }
    }

    // Built each time it opens, so it shows what's true now
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        switch needs() {
        case .success(let now) where now.isEmpty:
            add(menu, "Running")
        case .success(let now):
            add(menu, "Needs " + now.map(\.grant).joined(separator: ", "))
        // The whole reason is too long for a menu
        case .failure(let failure):
            add(menu, "Not running").toolTip = registerError ?? failure.message
        }
        if let seat = settings.seat {
            menu.addItem(.separator())
            header(menu, "Dictation")
            add(menu, "Copy Last Transcription") {
                switch hunchHost(["seat", "last"]) {
                case .success(let text):
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(
                        String(decoding: text, as: UTF8.self).trimmingCharacters(in: .newlines), forType: .string)
                case .failure(let failure):
                    self.showError(failure.message)
                }
            }
            settings.refreshHosts()
            speech(menu, seat)
            if let compute = seat.compute.map(split) {
                model(menu, seat, compute)
            }
            // One item, as Handy has it: redirect, or stop the redirect
            if seat.target == "focus" {
                add(menu, "Redirect to Window…") { self.pickWindow() }
            } else {
                add(menu, "Stop Redirect to \(describe(seat.target))") { _ = hunchHost(["seat", "target", "focus"]) }
            }
        }
        if let browser = decode(BrowserStatus.self, ["browser", "status"]) {
            menu.addItem(.separator())
            header(menu, "Browser")
            // What goes where, as for dictation
            let pages = browser.browsers.isEmpty
                ? add(menu, "No browser connected")
                : add(menu, browser.browsers.joined(separator: ", ") + " · pages to " + hostName(browser.serves))
            pages.image = NSImage(systemSymbolName: "globe", accessibilityDescription: "Browser")
            // Only the hosts the human allowed, as SEAT.md says
            let hosts = NSMenu()
            for host in ["localhost"] + browser.allow {
                let item = add(hosts, hostName(host)) {
                    if case .failure(let failure) = hunchHost(["browser", "serve", host]) {
                        self.showError(failure.message)
                    }
                }
                item.state = host == browser.serves ? .on : .off
            }
            hosts.addItem(.separator())
            add(hosts, "Allowed Hosts…") { self.showSettings(.browser) }
            pages.submenu = hosts
            if browser.browsers.isEmpty {
                add(menu, "Install for Firefox") {
                    if case .failure(let failure) = hunchHost(["browser", "install", "firefox"]) {
                        self.showError(failure.message)
                    }
                }
            }
        }
        menu.addItem(.separator())
        add(menu, "Settings…", key: ",") { self.showSettings() }
        menu.addItem(.separator())
        add(menu, "Quit", key: "q") { NSApp.terminate(nil) }
    }

    /// Where the speech goes, among the hosts the human allowed
    func speech(_ menu: NSMenu, _ seat: SeatState) {
        let chosen = seat.compute.map(split)?.host
        let hosts = (settings.roles.contains("compute") ? ["localhost"] : []) + settings.micAllow
        let speech = add(menu, chosen.map { "Speech to " + hostName($0) } ?? "Speech to…")
        speech.image = NSImage(systemSymbolName: "waveform", accessibilityDescription: "Speech")
        let computes = NSMenu()
        for host in hosts {
            let item = add(computes, hostName(host)) {
                if case .failure(let failure) = hunchHost(["seat", "compute", host, "local:"]) {
                    self.showError(failure.message)
                }
            }
            item.state = host == chosen ? .on : .off
        }
        computes.addItem(.separator())
        add(computes, "Allowed Hosts…") { self.showSettings(.dictation) }
        speech.submenu = computes
    }

    /// The model, on COMPUTE, as last asked: another host answers over ssh
    func model(_ menu: NSMenu, _ seat: SeatState, _ compute: (host: String, addr: String)) {
        let model = settings.models.first { $0.id == seat.model }
        // A loaded model takes about its file's size in memory
        let memory = seat.loading ? " · loading"
            : model?.loaded == true ? " · " + ByteCountFormatter.string(fromByteCount: model!.size, countStyle: .memory) : ""
        let item = add(menu, (model.map { $0.name ?? $0.id } ?? "No model") + memory)
        let models = NSMenu()
        for choice in settings.models {
            let entry = add(models, choice.name ?? choice.id) {
                if case .failure(let failure) = hunchHost(["seat", "model", choice.id]) {
                    self.showError(failure.message)
                }
            }
            entry.state = choice.id == seat.model ? .on : .off
        }
        item.submenu = models.items.isEmpty ? nil : models
        guard let model, !seat.loading else { return }
        let (host, addr) = compute
        if model.loaded {
            let eject = add(menu, "Eject") {
                DispatchQueue.global().async {
                    _ = hunchHost(["compute", addr, "unload"], on: host)
                    DispatchQueue.main.async { self.settings.refreshModels() }
                }
            }
            eject.image = NSImage(systemSymbolName: "eject", accessibilityDescription: "Eject")
        } else {
            add(menu, "Load") {
                DispatchQueue.global().async { _ = hunchHost(["compute", addr, "load", model.id], on: host) }
            }
        }
    }

    /// The app's own menu, never shown in the menu bar while it has no
    /// Dock icon, but where ⌘, ⌘W and ⌘Q work in its windows
    func mainMenu() -> NSMenu {
        let app = NSMenu()
        add(app, "Settings…", key: ",") { self.showSettings() }
        app.addItem(NSMenuItem(title: "Close", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w"))
        app.addItem(.separator())
        add(app, "Quit Hunch", key: "q") { NSApp.terminate(nil) }
        let item = NSMenuItem()
        item.submenu = app
        let main = NSMenu()
        main.addItem(item)
        return main
    }

    func header(_ menu: NSMenu, _ title: String) {
        if #available(macOS 14, *) {
            menu.addItem(.sectionHeader(title: title))
        } else {
            add(menu, title)
        }
    }

    @discardableResult
    func add(_ menu: NSMenu, _ title: String, key: String = "", action: (() -> Void)? = nil) -> NSMenuItem {
        let entry = NSMenuItem(title: title, action: nil, keyEquivalent: key)
        if let action {
            entry.representedObject = action
            entry.target = self
            entry.action = #selector(run(_:))
        }
        menu.addItem(entry)
        return entry
    }

    @objc func run(_ entry: NSMenuItem) {
        (entry.representedObject as? () -> Void)?()
    }
}

MainActor.assumeIsolated {
    let tray = Tray()
    NSApplication.shared.delegate = tray
    NSApplication.shared.run()
}

/// The app icon's slanted H in one color, as macOS tints menu bar icons,
/// in icon.swift's proportions
func trayIcon() -> NSImage {
    let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) { rect in
        let ctx = NSGraphicsContext.current!.cgContext
        ctx.translateBy(x: rect.midX, y: rect.midY)
        ctx.concatenate(CGAffineTransform(a: 1, b: 0, c: 0.14, d: 1, tx: 0, ty: 0))
        // icon.swift's H is 300 points tall
        let scale = 14.0 / 300
        let width = 250 * scale, height = 300 * scale, stem = 82 * scale, bar = 60 * scale
        ctx.fill(CGRect(x: -width / 2, y: -height / 2, width: stem, height: height))
        ctx.fill(CGRect(x: width / 2 - stem, y: -height / 2, width: stem, height: height))
        ctx.fill(CGRect(x: -width / 2, y: -bar / 2, width: width, height: bar))
        return true
    }
    image.isTemplate = true
    image.accessibilityDescription = "Hunch"
    return image
}
