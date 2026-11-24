// The tray of hunch on macOS: it runs hunch-seat as the human's service,
// shows what the seat does, and sets the seat from its settings window.

import AVFoundation
import AppKit
import ApplicationServices
import ServiceManagement
import SwiftUI

/// What went wrong, as the human reads it, with hunch-seat's exit status
struct Failure: Error {
  let message: String
  var status: Int32 = 1
}

/// pick's exit status when the human pressed Escape
let cancelled: Int32 = 5

let seatURL = Bundle.main.bundleURL.appendingPathComponent(
  "Contents/MacOS/hunch-seat")
let ninepURL = Bundle.main.bundleURL.appendingPathComponent(
  "Contents/MacOS/hunch-9p")
let hostURL = Bundle.main.bundleURL.appendingPathComponent(
  "Contents/MacOS/hunch-host")

/// ~/Library/Logs/Hunch/NAME.log, opened to append, for a program's stderr
func logFile(_ name: String) -> FileHandle {
  let logs = FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent("Library/Logs/Hunch")
  try? FileManager.default.createDirectory(
    at: logs, withIntermediateDirectories: true)
  let log = logs.appendingPathComponent("\(name).log")
  if !FileManager.default.fileExists(atPath: log.path) {
    FileManager.default.createFile(atPath: log.path, contents: nil)
  }
  let handle = try? FileHandle(forWritingTo: log)
  handle?.seekToEndOfFile()
  return handle ?? FileHandle.nullDevice
}

/// Where hunch keeps its runtime files, as hunch-host finds it
func hunchRoot() -> URL {
  let env = ProcessInfo.processInfo.environment
  let dir = ["XDG_RUNTIME_DIR", "XDG_CACHE_HOME"].compactMap { env[$0] }
    .first { !$0.isEmpty }
  return
    (dir.map { URL(fileURLWithPath: $0) }
    ?? FileManager.default.homeDirectoryForCurrentUser
    .appendingPathComponent(".cache"))
    .appendingPathComponent("hunch")
}

/// Runs this app's hunch-host in place of the one running, which may be an
/// earlier build's; once, since hunch-9p starts it again if it ends
func serveTree() {
  let pidFile = hunchRoot().appendingPathComponent("hunch-host.pid")
  let running = (try? String(contentsOf: pidFile, encoding: .utf8))
    .flatMap { Int32($0.trimmingCharacters(in: .whitespacesAndNewlines)) }
  if let pid = running, kill(pid, SIGTERM) == 0 {
    // It lets go of its lock as it ends
    for _ in 0..<20 where kill(pid, 0) == 0 {
      usleep(50_000)
    }
  }
  let process = Process()
  process.executableURL = hostURL
  process.standardError = logFile("hunch-host")
  process.standardOutput = FileHandle.nullDevice
  try? process.run()
}

/// Runs the program at URL, with INPUT on its stdin; its stdout
func run(_ url: URL, _ arguments: [String], input: Data? = nil) -> Result<
  Data, Failure
> {
  let process = Process()
  process.executableURL = url
  process.arguments = arguments
  let out = Pipe()
  let err = Pipe()
  let into = Pipe()
  process.standardOutput = out
  process.standardError = err
  process.standardInput = input == nil ? FileHandle.nullDevice : into
  do {
    try process.run()
  } catch {
    return .failure(
      Failure(
        message:
          "can't run \(url.lastPathComponent): \(error.localizedDescription)"
      ))
  }
  if let input {
    into.fileHandleForWriting.write(input)
    try? into.fileHandleForWriting.close()
  }
  let data = out.fileHandleForReading.readDataToEndOfFile()
  process.waitUntilExit()
  guard process.terminationStatus == 0 else {
    // The first line, from where hunch-seat says why: the rest is
    // usage, too long for a menu or the pill
    let line =
      String(
        decoding: err.fileHandleForReading.readDataToEndOfFile(),
        as: UTF8.self
      )
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .split(separator: "\n", maxSplits: 1).first.map(String.init) ?? ""
    let said =
      line.range(of: "hunch-seat: ", options: .backwards).map {
        String(line[$0.upperBound...])
      } ?? line
    return .failure(
      Failure(message: said, status: process.terminationStatus))
  }
  return .success(data)
}

/// The PATH of the human's shell: an app started from Finder or at login
/// gets only the system's, without tmux and hunch, which the service and
/// hunch-host run. A marker sets it apart from what the shell's startup
/// prints.
func shellPath() -> String? {
  let shell = ProcessInfo.processInfo.environment["SHELL"] ?? "/bin/zsh"
  let marker = "PATH="
  guard
    case .success(let data) = run(
      URL(fileURLWithPath: shell),
      ["-ilc", "printf '\\n\(marker)%s' \"$PATH\""]),
    let line = String(decoding: data, as: UTF8.self).components(
      separatedBy: "\n"
    ).last,
    line.hasPrefix(marker)
  else { return nil }
  return String(line.dropFirst(marker.count))
}

/// Reads FILE of the seat in the host's workspace, through the hunch-9p
/// next to this app
func readSeat(_ file: String) -> Result<Data, Failure> {
  readTree("seat/" + file)
}

/// PATH of the host's workspace
func readTree(_ path: String) -> Result<Data, Failure> {
  run(ninepURL, ["read", path])
}

/// Writes TEXT to PATH of the host's workspace, an action
func writeTree(_ path: String, _ text: String) -> Result<Data, Failure> {
  run(ninepURL, ["write", path], input: Data(text.utf8))
}

/// Writes TEXT to FILE of the seat, an action
func writeSeat(_ file: String, _ text: String) -> Result<Data, Failure> {
  writeTree("seat/" + file, text)
}

/// Writes LINE to the seat's ctl: one of the seat's commands
func seatCtl(_ line: String) -> Result<Data, Failure> {
  writeSeat("ctl", line)
}

/// PATH of the host's workspace, as JSON
func decode<T: Decodable>(_ type: T.Type, _ path: String) -> T? {
  guard case .success(let data) = readTree(path) else { return nil }
  return try? JSONDecoder().decode(type, from: data)
}

struct Need: Decodable {
  let `for`: String
  let grant: String
  /// Set when needs lists every grant
  let granted: Bool?
}

/// Every grant hunch-seat uses, each with whether it has it
func needs() -> Result<[Need], Failure> {
  readSeat("needs").flatMap { data in
    Result { try JSONDecoder().decode([Need].self, from: data) }
      .mapError {
        Failure(
          message: "the seat's needs aren't JSON: \($0.localizedDescription)")
      }
  }
}

/// Has macOS ask the human for what hunch-seat lacks, in its name: only
/// its own process can, so this one goes to it rather than to its files
func askForNeeds() {
  _ = run(seatURL, ["needs", "--ask"])
}

/// Where System Settings lets the human grant each thing, and its icon
let panes = [
  "Screen Recording": (
    "Privacy_ScreenCapture", "rectangle.dashed.badge.record"
  ),
  "Accessibility": ("Privacy_Accessibility", "accessibility"),
  "Microphone": ("Privacy_Microphone", "mic"),
  "Camera": ("Privacy_Camera", "camera"),
]

/// macOS ties hunch-seat's grants to this app, and lists an app in System
/// Settings only once the app itself asks; the service asks too, for a
/// grant that would land on it. macOS prompts once; after that, only
/// System Settings grants.
func ask(_ grant: String) {
  // The microphone prompt is the grant itself, no System Settings needed
  if grant == "Microphone",
    AVCaptureDevice.authorizationStatus(for: .audio) == .notDetermined
  {
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
  askForNeeds()
  let pane = panes[grant].map { "?\($0.0)" } ?? ""
  NSWorkspace.shared.open(
    URL(
      string:
        "x-apple.systempreferences:com.apple.preference.security\(pane)"
    )!)
}

/// The seat as watch prints it
struct SeatState: Decodable {
  let recording: Bool
  let opening: Bool
  let transcribing: Bool
  let loading: Bool
  /// The human picks the window dictation goes to
  let picking: Bool
  let model: String?
  let target: String
  let submit: Bool
  let key: String
  let preload: Bool
  /// Minutes the model stays in GPU memory while idle; nil is always
  let warm: Int?
  let error: String?
}

/// The namespace, which only the human writes, here through the app; it
/// mounts the ENDPOINT inference/ is, as INFERENCE.md in hunch says
enum Namespace {
  static var url: URL {
    let config =
      ProcessInfo.processInfo.environment["XDG_CONFIG_HOME"].map {
        URL(fileURLWithPath: $0)
      }
      ?? FileManager.default.homeDirectoryForCurrentUser
      .appendingPathComponent(".config")
    return config.appendingPathComponent("hunch/namespace")
  }

  static func lines() -> [String] {
    let text = (try? String(contentsOf: url, encoding: .utf8)) ?? ""
    return text.split(separator: "\n", omittingEmptySubsequences: false)
      .map(String.init)
  }

  /// The words of a mount on inference/, if LINE is one
  static func mount(_ line: String) -> String? {
    let words = line.split(separator: " ").map(String.init)
    return words.count == 3 && words[0] == "mount" && words[2] == "/inference"
      ? words[1] : nil
  }

  /// The ENDPOINT mounted: the last mount, else local:
  static func mounted() -> String {
    lines().compactMap(mount).last ?? "local:"
  }

  /// Mounts ENDPOINT, in place of the mounts before, keeping the rest
  static func mount(endpoint: String) throws {
    var lines = lines().filter { mount($0) == nil }
    while lines.last == "" { lines.removeLast() }
    if endpoint != "local:" {
      lines.append("mount \(endpoint) /inference")
    }
    try FileManager.default.createDirectory(
      at: url.deletingLastPathComponent(),
      withIntermediateDirectories: true)
    try (lines.joined(separator: "\n") + "\n").write(
      to: url, atomically: true, encoding: .utf8)
  }
}

/// An item of the seat's menu, as SEAT.md's Files say
struct MenuItem: Decodable {
  let title: String?
  /// A separator, with a heading unless it's empty
  let separator: String?
  let checked: Bool?
  /// What it does: write a line to the seat's ctl, copy a file of the
  /// seat, or open a page of the settings, the first for an empty one
  let ctl: String?
  let copy: String?
  let settings: String?
  let items: [MenuItem]?
}

let showInDockKey = "showInDock"

/// Puts the app in the Dock or takes it out; the tray icon stays
@MainActor
func showInDock(_ shown: Bool) {
  NSApp.setActivationPolicy(shown ? .regular : .accessory)
}

/// What the app's windows show
@MainActor
final class Settings: ObservableObject {
  struct Grant: Identifiable {
    let need: Need
    var allowed: Bool
    var id: String { need.grant }
  }

  @Published var grants: [Grant] = []
  @Published var error: String?
  @Published var seat: SeatState?
  /// The ENDPOINT the namespace mounts on inference/
  @Published var mounted = Namespace.mounted()
  /// The models of inference/
  @Published var models: [SeatModel] = []
  @Published var inDock = UserDefaults.standard.bool(forKey: showInDockKey)

  var missing: Bool { grants.contains { !$0.allowed } }

  /// Reads the seat off the main thread, so a slow answer never holds up
  /// the menu or the settings
  func refresh() {
    DispatchQueue.global().async {
      let needs = needs()
      DispatchQueue.main.async {
        switch needs {
        case .success(let all):
          self.error = nil
          self.grants = all.map {
            Grant(need: $0, allowed: $0.granted ?? false)
          }
        case .failure(let failure):
          self.error = failure.message
        }
      }
    }
    refreshHosts()
  }

  /// Where speech goes, and the models there
  func refreshHosts() {
    mounted = Namespace.mounted()
    DispatchQueue.global().async {
      // A server's come over the network
      let models = decode([SeatModel].self, "inference/models") ?? []
      DispatchQueue.main.async { self.models = models }
    }
  }

  func mount(_ endpoint: String) {
    do {
      try Namespace.mount(endpoint: endpoint)
    } catch {
      self.error = error.localizedDescription
    }
    refreshHosts()
  }
}

/// Where speech goes: This Mac, or a server by its URL, with the key this
/// host logs in with, which hunch-host keeps in the Keychain; the key's
/// field shows none, since it isn't read back
struct InferenceSection: View {
  @ObservedObject var settings: Settings
  @State private var server = ""
  @State private var key = ""
  @State private var said: String?
  /// The human picked a server, which isn't mounted until they give it
  @State private var picking = false

  var local: Bool { settings.mounted == "local:" && !picking }

  var body: some View {
    SwiftUI.Section {
      Picker(
        "Transcribe on",
        selection: Binding(
          get: { local ? "local:" : "server" },
          set: {
            picking = $0 == "server"
            if !picking { settings.mount("local:") }
          })
      ) {
        Text("This Mac").tag("local:")
        Text("A Server").tag("server")
      }
      if !local {
        // verbatim, or the URL shows as a link
        TextField(
          "Server", text: $server,
          prompt: Text(verbatim: "http://thinkstation:8080/v1")
        )
        .onSubmit(mount)
        HStack {
          SecureField(
            "Key", text: $key, prompt: Text("if the server asks for one"))
          Button("Keep", action: keep).disabled(key.isEmpty || local)
        }
      }
      if let said { Text(said).foregroundStyle(.secondary) }
    } footer: {
      Text(
        "This Mac runs its own models; a server is an OpenAI-compatible "
          + "one. Press Return to keep the server."
      )
      .foregroundStyle(.secondary)
    }
    .onAppear { server = local ? "" : settings.mounted }
    .onChange(of: settings.mounted) { _ in picking = false }
  }

  func mount() {
    let url = server.trimmingCharacters(in: .whitespaces)
    guard let parts = URLComponents(string: url),
      ["http", "https"].contains(parts.scheme), parts.host != nil
    else {
      said = "\(url) isn't a server's URL"
      return
    }
    settings.mount(url)
    picking = false
    said = nil
  }

  func keep() {
    switch writeTree("inference/login", key + "\n") {
    case .success:
      key = ""
      said = "Kept the key in the Keychain"
    case .failure(let failure):
      said = failure.message
    }
  }
}

/// A model as inference/models lists it
struct SeatModel: Decodable, Hashable {
  let id: String
  let name: String?
  let inputs: [String]?
  let outputs: [String]?

  /// Takes audio and answers text: it transcribes
  var transcribes: Bool {
    (inputs ?? []).contains { $0.hasPrefix("audio/") }
      && (outputs ?? []).contains { $0.hasPrefix("text/") }
  }
}

struct GrantRows: View {
  @ObservedObject var settings: Settings

  var body: some View {
    ForEach(settings.grants) { grant in
      LabeledContent {
        if grant.allowed {
          Image(systemName: "checkmark.circle.fill").foregroundStyle(
            .green)
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
        Text("Hold the key and speak; Escape cancels.").foregroundStyle(
          .secondary)
      }
      SwiftUI.Section {
        GrantRows(settings: settings)
      } header: {
        Text("Hunch needs a few permissions")
      } footer: {
        Text(
          "macOS asks before any app can see your windows, type, or hear you "
            + "or your keys. Nothing leaves this Mac."
        )
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

/// The dictation keys hunch-seat knows, by its names, as the human reads
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
          Text(
            dictationKeys.first { $0.name == settings.seat?.key }?
              .label ?? "None")
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
    monitor = NSEvent.addLocalMonitorForEvents(matching: .flagsChanged) {
      event in
      if let key = dictationKeys.first(where: { $0.code == event.keyCode }
      ) {
        _ = seatCtl("key \(key.name)")
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

/// The app's version, as its Info.plist says
let version =
  Bundle.main.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""

struct SettingsView: View {
  enum Tab: CaseIterable {
    case general, dictation, inference, permissions, about
  }

  @ObservedObject var settings: Settings
  let tab: Tab

  var body: some View {
    switch tab {
    case .general: general
    case .dictation: dictation
    case .inference: inference
    case .permissions: permissions
    case .about: about
    }
  }

  /// Sets the seat through its ctl; the change comes back by watch
  nonisolated func seat(_ arguments: String...) -> @Sendable (String) -> Void {
    { value in _ = seatCtl((arguments + [value]).joined(separator: " ")) }
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

  /// The models of the ENDPOINT in use that turn speech into text
  var transcribing: [SeatModel] { settings.models.filter(\.transcribes) }

  var dictation: some View {
    page {
      SwiftUI.Section {
        Picker(
          "Model",
          selection: Binding(
            get: { settings.seat?.model ?? "" },
            set: { seat("model")($0) })
        ) {
          let models = transcribing
          if !models.contains(where: { $0.id == settings.seat?.model }) {
            Text("None").tag("")
          }
          ForEach(models, id: \.id) {
            Text($0.name ?? $0.id).tag($0.id)
          }
        }
      } footer: {
        if transcribing.isEmpty {
          Text("No model of Inference says it takes speech.")
            .foregroundStyle(.secondary)
        }
      }
      SwiftUI.Section {
        KeyRow(settings: settings)
        Toggle(
          "Press Enter after the text",
          isOn: Binding(
            get: { settings.seat?.submit ?? true },
            set: { seat("submit")($0 ? "on" : "off") }
          )
        )
        Toggle(
          "Load the model at start",
          isOn: Binding(
            get: { settings.seat?.preload ?? true },
            set: { seat("preload")($0 ? "on" : "off") }
          )
        )
        .help(
          "Off loads it at the first dictation, and frees its memory now"
        )
        Picker(
          "Keep the model ready",
          selection: Binding(
            get: {
              settings.seat?.warm.map(String.init) ?? "always"
            },
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
        .help(
          "How long the model stays in GPU memory while you don't dictate; "
            + "after that, the next dictation waits a moment"
        )
      } footer: {
        Text("Hold the key and speak; Escape cancels.").foregroundStyle(
          .secondary)
      }
    }
  }

  var inference: some View {
    page {
      InferenceSection(settings: settings)
    }
  }

  var about: some View {
    page {
      SwiftUI.Section {
        HStack(spacing: 12) {
          Image(nsImage: NSApp.applicationIconImage).resizable()
            .frame(width: 48, height: 48)
          VStack(alignment: .leading) {
            Text("Hunch").font(.headline)
            Text("Version \(version)")
              .foregroundStyle(.secondary)
          }
        }
      }
      SwiftUI.Section("Ideas") {
        credit(
          "Vannevar Bush", "As We May Think, 1945",
          "https://www.theatlantic.com/magazine/archive/1945/07/as-we-may-think/303881/"
        )
        credit(
          "J. C. R. Licklider", "Man-Computer Symbiosis, 1960",
          "https://groups.csail.mit.edu/medg/people/psz/Licklider.html"
        )
        credit(
          "Douglas Engelbart", "Augmenting Human Intellect, 1962",
          "https://www.dougengelbart.org/")
      }
      SwiftUI.Section("Built on") {
        credit(
          "ggml", "Georgi Gerganov and its authors",
          "https://github.com/ggml-org/ggml")
        credit(
          "llama.cpp", "Its authors",
          "https://github.com/ggml-org/llama.cpp")
        credit(
          "transcribe.cpp", "Its authors",
          "https://github.com/handy-computer/transcribe.cpp")
      }
      SwiftUI.Section("Inspiration") {
        credit(
          "Pipit", "The first dictation app that got Hunch going",
          "https://www.pipitvoice.com")
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
      ("Dictation", "waveform", .dictation),
      ("Inference", "network", .inference),
      ("Permissions", "lock.shield", .permissions),
      ("About", "info.circle", .about),
    ]
    for (title, symbol, tab) in pages {
      let host = NSHostingController(
        rootView: SettingsView(settings: settings, tab: tab))
      // The window takes each tab's own height
      host.sizingOptions = .preferredContentSize
      let item = NSTabViewItem(viewController: host)
      item.label = title
      item.image = NSImage(
        systemSymbolName: symbol, accessibilityDescription: title)
      tabs.addTabViewItem(item)
    }
    return tabs
  }

  /// A tab: a grouped form, with hunch-seat's error on top if it has one
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
  panel.collectionBehavior = [
    .canJoinAllSpaces, .fullScreenAuxiliary, .stationary,
  ]
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
    guard mode != .hidden, let screen = NSScreen.main,
      let view = panel.contentView
    else {
      panel.orderOut(nil)
      return
    }
    let size = view.fittingSize
    let frame = screen.visibleFrame
    panel.setFrame(
      NSRect(
        x: frame.midX - size.width / 2, y: frame.minY + 48,
        width: size.width, height: size.height),
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
          .animation(
            .easeInOut(duration: 0.7).repeatForever(), value: pulse
          )
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
        Image(systemName: "exclamationmark.triangle.fill")
          .foregroundStyle(.yellow)
        Text(message).lineLimit(2).frame(
          maxWidth: 380, alignment: .leading)
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
/// in hunch-seat takes the window under the click by the same rule: the
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
    timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 30, repeats: true) {
      _ in
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
      let list = CGWindowListCopyWindowInfo(
        [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
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
      return NSRect(
        x: rect.minX, y: main.frame.height - rect.maxY,
        width: rect.width, height: rect.height)
    }
    return nil
  }
}

@MainActor
final class Tray: NSObject, NSApplicationDelegate, NSMenuDelegate,
  NSWindowDelegate
{
  let item = NSStatusBar.system.statusItem(
    withLength: NSStatusItem.squareLength)
  var service: Process?
  var quitting = false
  let overlay = Overlay()
  let outline = Outline()
  let settings = Settings()
  var registerError: String?
  var settingsWindow: NSWindow?
  var permissionsWindow: NSWindow?
  var timer: Timer?
  var errorShown: String?
  /// The seat's menu as last read, so the menu opens without waiting
  var menuItems: [MenuItem]?

  func applicationShouldHandleReopen(
    _ sender: NSApplication, hasVisibleWindows: Bool
  ) -> Bool {
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
    if let path = shellPath() {
      setenv("PATH", path, 1)
    }
    serveTree()
    serve()
    watchSeat()
    if case .success(let all) = needs(), all.allSatisfy({ $0.granted == true })
    {
      return
    }
    showPermissions()
  }

  /// Runs hunch-seat serve for as long as the app runs, again if it
  /// ends; a service already running, e.g. under launchd, makes it end
  func serve() {
    let process = Process()
    process.executableURL = seatURL
    process.arguments = ["serve"]
    process.standardError = logFile("hunch-seat")
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
      registerError =
        "can't run hunch-seat: \(error.localizedDescription)"
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
    process.executableURL = seatURL
    process.arguments = ["seat", "watch"]
    let out = Pipe()
    process.standardOutput = out
    process.standardError = FileHandle.nullDevice
    // A thread of its own reads the lines, so none waits on the main one
    Thread {
      for line in LineReader(out.fileHandleForReading) {
        if let state = try? JSONDecoder().decode(
          SeatState.self, from: line)
        {
          DispatchQueue.main.async { self.seatChanged(state) }
        }
      }
    }.start()
    process.terminationHandler = { _ in
      DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
        self.watchSeat()
      }
    }
    try? process.run()
  }

  func seatChanged(_ state: SeatState) {
    // The icon stays: macOS's own sign shows the microphone records
    settings.seat = state
    refreshMenu()
    if state.picking {
      overlay.show(.picking)
      outline.start()
      return
    }
    outline.stop()
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

  /// A window of the app's, made once and shown in front
  func show(
    _ title: String, _ existing: NSWindow?,
    _ content: () -> NSViewController
  ) -> NSWindow {
    if existing == nil {
      settings.refresh()
      // Grants happen in System Settings, so the windows keep looking
      if timer == nil {
        timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { _ in
          MainActor.assumeIsolated { self.tick() }
        }
      }
    }
    let window =
      existing
      ?? {
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
    settingsWindow = show("Settings", settingsWindow) {
      SettingsView.tabs(settings)
    }
    if let tab,
      let tabs = settingsWindow?.contentViewController
        as? NSTabViewController
    {
      tabs.selectedTabViewItemIndex =
        SettingsView.Tab.allCases.firstIndex(of: tab) ?? 0
    }
  }

  func showPermissions() {
    permissionsWindow = show("Hunch", permissionsWindow) {
      NSHostingController(rootView: PermissionsView(settings: settings))
    }
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

  /// Reads the seat's menu in the background
  func refreshMenu() {
    DispatchQueue.global().async {
      let read = readSeat("menu").flatMap { data in
        Result { try JSONDecoder().decode([MenuItem].self, from: data) }
          .mapError {
            Failure(message: "the seat's menu isn't JSON: \($0)")
          }
      }
      DispatchQueue.main.async {
        self.menuItems = try? read.get()
      }
    }
  }

  // Drawn each time it opens, from the seat's menu as last read
  func menuNeedsUpdate(_ menu: NSMenu) {
    menu.removeAllItems()
    if let items = menuItems {
      draw(items, in: menu)
    } else {
      add(menu, "Not running").toolTip = registerError
      menu.addItem(.separator())
      add(menu, "Settings…", key: ",") { self.showSettings() }
    }
    menu.addItem(.separator())
    add(menu, "Quit", key: "q") { NSApp.terminate(nil) }
    refreshMenu()
  }

  func draw(_ items: [MenuItem], in menu: NSMenu) {
    for item in items {
      if let separator = item.separator {
        if separator.isEmpty {
          menu.addItem(.separator())
        } else {
          header(menu, separator)
        }
        continue
      }
      let acts =
        item.ctl != nil || item.copy != nil || item.settings != nil
      let entry = add(
        menu, item.title ?? "", key: item.settings == "" ? "," : "",
        action: acts ? { self.act(item) } : nil)
      entry.state = item.checked == true ? .on : .off
      if let items = item.items {
        let submenu = NSMenu()
        draw(items, in: submenu)
        entry.submenu = submenu
      }
    }
  }

  func act(_ item: MenuItem) {
    if let line = item.ctl {
      // In the background: a pick waits for a click, a load for the model
      DispatchQueue.global().async {
        let done = seatCtl(line)
        DispatchQueue.main.async {
          if case .failure(let failure) = done {
            self.showError(failure.message)
          }
          self.refreshMenu()
        }
      }
    } else if let file = item.copy {
      switch readSeat(file) {
      case .success(let text):
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(
          String(decoding: text, as: UTF8.self).trimmingCharacters(
            in: .newlines), forType: .string)
      case .failure(let failure):
        showError(failure.message)
      }
    } else if let page = item.settings {
      showSettings(SettingsView.Tab.allCases.first { "\($0)" == page })
    }
  }

  /// The app's own menu, never shown in the menu bar while it has no
  /// Dock icon, but where ⌘, ⌘W and ⌘Q work in its windows
  func mainMenu() -> NSMenu {
    let app = NSMenu()
    add(app, "Settings…", key: ",") { self.showSettings() }
    app.addItem(
      NSMenuItem(
        title: "Close", action: #selector(NSWindow.performClose(_:)),
        keyEquivalent: "w"))
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
  func add(
    _ menu: NSMenu, _ title: String, key: String = "",
    action: (() -> Void)? = nil
  ) -> NSMenuItem {
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
  let image = NSImage(size: NSSize(width: 18, height: 18), flipped: false) {
    rect in
    let ctx = NSGraphicsContext.current!.cgContext
    ctx.translateBy(x: rect.midX, y: rect.midY)
    ctx.concatenate(
      CGAffineTransform(a: 1, b: 0, c: 0.14, d: 1, tx: 0, ty: 0))
    // icon.swift's H is 300 points tall
    let scale = 14.0 / 300
    let width = 250 * scale, height = 300 * scale, stem = 82 * scale,
      bar = 60 * scale
    ctx.fill(
      CGRect(x: -width / 2, y: -height / 2, width: stem, height: height))
    ctx.fill(
      CGRect(
        x: width / 2 - stem, y: -height / 2, width: stem, height: height
      ))
    ctx.fill(CGRect(x: -width / 2, y: -bar / 2, width: width, height: bar))
    return true
  }
  image.isTemplate = true
  image.accessibilityDescription = "Hunch"
  return image
}
