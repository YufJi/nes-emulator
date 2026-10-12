import MetalKit
import UIKit
import UniformTypeIdentifiers

// A minimal, playable app: pick a `.nes` file, then play it on the screen with an
// on-screen pad.
//
// UIKit rather than SwiftUI on purpose — there is nothing here that needs more
// than a Metal view and some buttons, and a game loop that owns its own timing
// reads better without a view framework in the way.

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    var window: UIWindow?

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil,
    ) -> Bool {
        let window = UIWindow(frame: UIScreen.main.bounds)
        window.rootViewController = GameViewController()
        window.makeKeyAndVisible()
        self.window = window
        return true
    }
}

/// Owns the console and the three drivers: screen, audio, display link.
final class GameViewController: UIViewController {
    private let emulator = Emulator()
    private lazy var audio = AudioOutput(emulator: emulator)
    private lazy var display = DisplayLinkDriver(emulator: emulator)

    private var screen: MetalScreen!
    private let status = UILabel()
    private let pickButton = UIButton(type: .system)

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .black
        buildScreen()
        buildOverlay()
        wireEmulator()

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(releaseAllButtons),
            name: UIApplication.didEnterBackgroundNotification,
            object: nil,
        )
        showStatus("选择一个 .nes 文件开始")
    }

    // MARK: - Screen

    private func buildScreen() {
        guard let device = MTLCreateSystemDefaultDevice() else {
            showStatus("这个设备没有 Metal")
            return
        }
        let screen = MetalScreen(emulator: emulator, device: device)
        screen.frame = view.bounds
        screen.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        view.addSubview(screen)
        self.screen = screen
    }

    private func wireEmulator() {
        let screen = self.screen
        // Called on the emulation queue: the upload happens there, no hop.
        emulator.onFrame = { [weak screen] frame in screen?.present(frame) }
        emulator.onFramesPerSecond = { [weak self] fps in
            DispatchQueue.main.async { self?.status.text = String(format: "%.1f fps", fps) }
        }
        emulator.onFatalError = { [weak self] detail in
            DispatchQueue.main.async {
                self?.display.stop()
                self?.presentAlert(
                    title: "内核异常，已停止",
                    message: "ROM 可能已损坏，换一个即可继续。\n\n\(detail)",
                )
            }
        }
    }

    // MARK: - Overlay

    private func buildOverlay() {
        status.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        status.textColor = .white
        status.translatesAutoresizingMaskIntoConstraints = false

        pickButton.setTitle("选择 ROM", for: .normal)
        pickButton.titleLabel?.font = .systemFont(ofSize: 14, weight: .semibold)
        pickButton.translatesAutoresizingMaskIntoConstraints = false
        pickButton.addTarget(self, action: #selector(pickRom), for: .touchUpInside)

        let topBar = UIView()
        topBar.backgroundColor = UIColor.black.withAlphaComponent(0.45)
        topBar.translatesAutoresizingMaskIntoConstraints = false
        topBar.addSubview(status)
        topBar.addSubview(pickButton)

        let pad = buildPad()
        pad.translatesAutoresizingMaskIntoConstraints = false

        view.addSubview(topBar)
        view.addSubview(pad)

        NSLayoutConstraint.activate([
            topBar.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            topBar.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            topBar.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            topBar.heightAnchor.constraint(equalToConstant: 36),

            status.leadingAnchor.constraint(equalTo: topBar.leadingAnchor, constant: 12),
            status.centerYAnchor.constraint(equalTo: topBar.centerYAnchor),

            pickButton.trailingAnchor.constraint(equalTo: topBar.trailingAnchor, constant: -12),
            pickButton.centerYAnchor.constraint(equalTo: topBar.centerYAnchor),
            pickButton.leadingAnchor.constraint(greaterThanOrEqualTo: status.trailingAnchor, constant: 8),

            pad.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            pad.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            pad.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor),
        ])
    }

    private func buildPad() -> UIView {
        let container = UIView()

        let dpad = UIStackView(arrangedSubviews: [
            centered(NesButton.up, "▲"),
            row([button(NesButton.left, "◀"), spacer(), button(NesButton.right, "▶")]),
            centered(NesButton.down, "▼"),
        ])
        dpad.axis = .vertical
        dpad.alignment = .center
        dpad.spacing = 6

        let face = UIStackView(arrangedSubviews: [
            row([button(NesButton.b, "B"), button(NesButton.a, "A")]),
            row([button(NesButton.select, "SELECT", width: 88), button(NesButton.start, "START", width: 88)]),
        ])
        face.axis = .vertical
        face.alignment = .center
        face.spacing = 8

        let line = UIStackView(arrangedSubviews: [dpad, face])
        line.axis = .horizontal
        line.distribution = .equalSpacing
        line.alignment = .center
        line.translatesAutoresizingMaskIntoConstraints = false
        container.addSubview(line)

        NSLayoutConstraint.activate([
            line.leadingAnchor.constraint(equalTo: container.leadingAnchor, constant: 20),
            line.trailingAnchor.constraint(equalTo: container.trailingAnchor, constant: -20),
            line.topAnchor.constraint(equalTo: container.topAnchor, constant: 8),
            line.bottomAnchor.constraint(equalTo: container.bottomAnchor, constant: -8),
        ])
        return container
    }

    private func row(_ views: [UIView]) -> UIStackView {
        let stack = UIStackView(arrangedSubviews: views)
        stack.axis = .horizontal
        stack.spacing = 6
        stack.alignment = .center
        return stack
    }

    private func centered(_ nesButton: UInt8, _ title: String) -> UIView {
        let stack = row([button(nesButton, title)])
        stack.alignment = .center
        return stack
    }

    private func spacer() -> UIView {
        let view = UIView()
        view.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            view.widthAnchor.constraint(equalToConstant: 56),
            view.heightAnchor.constraint(equalToConstant: 56),
        ])
        return view
    }

    private func button(_ nesButton: UInt8, _ title: String, width: CGFloat = 56) -> UIButton {
        let button = PadButton(type: .system)
        button.nesButton = nesButton
        button.setTitle(title, for: .normal)
        button.titleLabel?.font = .monospacedSystemFont(ofSize: 13, weight: .semibold)
        button.setTitleColor(.white, for: .normal)
        button.backgroundColor = UIColor.white.withAlphaComponent(0.12)
        button.layer.cornerRadius = 10
        button.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            button.widthAnchor.constraint(equalToConstant: width),
            button.heightAnchor.constraint(equalToConstant: 56),
        ])
        button.addTarget(self, action: #selector(press(_:)), for: .touchDown)
        button.addTarget(
            self,
            action: #selector(release(_:)),
            for: [.touchUpInside, .touchUpOutside, .touchCancel, .touchDragExit],
        )
        return button
    }

    // MARK: - Actions

    @objc private func press(_ sender: PadButton) {
        emulator.setButton(sender.nesButton, pressed: true)
    }

    @objc private func release(_ sender: PadButton) {
        emulator.setButton(sender.nesButton, pressed: false)
    }

    @objc private func releaseAllButtons() {
        emulator.releaseAllButtons()
    }

    @objc private func pickRom() {
        let types = [UTType(filenameExtension: "nes") ?? .data]
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: types)
        picker.delegate = self
        present(picker, animated: true)
    }

    private func load(_ url: URL) {
        // Files outside the app's sandbox need this.
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        do {
            let rom = try Data(contentsOf: url)
            try emulator.load(rom: rom)
            emulator.audio.flush()
            startAudioIfNeeded()
            display.stop()
            display.start()
            title = url.lastPathComponent
        } catch {
            presentAlert(title: "无法加载", message: "\(error)")
        }
    }

    private func startAudioIfNeeded() {
        guard !audio.isRunning else { return }
        do {
            try audio.start()
        } catch {
            // Play without sound rather than not at all.
            presentAlert(title: "音频启动失败", message: "继续静音运行：\(error.localizedDescription)")
        }
    }

    private func showStatus(_ text: String) {
        status.text = text
    }

    private func presentAlert(title: String, message: String) {
        let alert = UIAlertController(title: title, message: message, preferredStyle: .alert)
        alert.addAction(UIAlertAction(title: "好", style: .default))
        if presentedViewController == nil {
            present(alert, animated: true)
        }
    }
}

extension GameViewController: UIDocumentPickerDelegate {
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        guard let url = urls.first else { return }
        load(url)
    }
}

/// A pad key that knows which console button it stands for.
private final class PadButton: UIButton {
    var nesButton: UInt8 = 0
}
