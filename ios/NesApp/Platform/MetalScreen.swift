#if os(iOS)
import MetalKit

/// Draws the console with Metal, reading the picture straight out of the core.
///
/// **What is and is not zero-copy here.** The pixels are read through
/// ``FrameSource``, which points into the core's own buffer: no `[UInt32]` array
/// is built, and no per-pixel work happens on the CPU. The one remaining copy is
/// the upload into the texture (`replace(region:)`) — a GPU upload is inherent
/// while the CPU owns the pixels. Removing even that would mean having the core
/// render into memory the GPU already maps (an `IOSurface`), which it does not do
/// today and which Metal would additionally require to be page-aligned.
///
/// The shader is compiled at runtime from a string so the package stays
/// buildable with SwiftPM, which does not compile `.metal` files.
public final class MetalScreen: MTKView {
    private let emulator: Emulator
    private let commandQueue: MTLCommandQueue
    private let texture: MTLTexture
    private var pipeline: MTLRenderPipelineState?
    private var sampler: MTLSamplerState?

    public init(emulator: Emulator, device: MTLDevice) {
        self.emulator = emulator
        self.commandQueue = device.makeCommandQueue()!

        let descriptor = MTLTextureDescriptor.texture2DDescriptor(
            pixelFormat: .bgra8Unorm,
            width: 256,
            height: 240,
            mipmapped: false,
        )
        descriptor.usage = [.shaderRead]
        descriptor.storageMode = .shared
        self.texture = device.makeTexture(descriptor: descriptor)!

        super.init(frame: .zero, device: device)

        colorPixelFormat = .bgra8Unorm
        isPaused = true
        enableSetNeedsDisplay = true
        delegate = self
    }

    @available(*, unavailable)
    required init(coder: NSCoder) {
        fatalError("init(coder:) is not used")
    }

    /// Upload the newest picture. Called from the emulation queue.
    ///
    /// `replace(region:)` takes the bytes straight from the core's buffer; the
    /// texture never sees an intermediate allocation.
    public func present(_ frame: FrameSource) {
        frame.pixels.withUnsafeBytes { bytes in
            guard let base = bytes.baseAddress else { return }
            texture.replace(
                region: MTLRegionMake2D(0, 0, frame.width, frame.height),
                mipmapLevel: 0,
                withBytes: base,
                bytesPerRow: frame.bytesPerRow,
            )
        }
        DispatchQueue.main.async { [weak self] in
            self?.setNeedsDisplay()
        }
    }

    private func makePipelineIfNeeded() -> MTLRenderPipelineState? {
        if let pipeline { return pipeline }
        guard
            let device,
            let library = try? device.makeLibrary(source: Self.shaderSource, options: nil),
            let vertex = library.makeFunction(name: "nes_vertex"),
            let fragment = library.makeFunction(name: "nes_fragment")
        else {
            return nil
        }

        let descriptor = MTLRenderPipelineDescriptor()
        descriptor.vertexFunction = vertex
        descriptor.fragmentFunction = fragment
        descriptor.colorAttachments[0].pixelFormat = colorPixelFormat
        pipeline = try? device.makeRenderPipelineState(descriptor: descriptor)

        let samplerDescriptor = MTLSamplerDescriptor()
        samplerDescriptor.minFilter = .nearest
        samplerDescriptor.magFilter = .nearest
        sampler = device.makeSamplerState(descriptor: samplerDescriptor)

        return pipeline
    }

    /// A full-screen triangle: three vertices cover the target with no vertex
    /// buffer at all.
    private static let shaderSource = """
    #include <metal_stdlib>
    using namespace metal;

    struct VertexOut {
        float4 position [[position]];
        float2 uv;
    };

    vertex VertexOut nes_vertex(uint id [[vertex_id]],
                                constant float2 &viewport [[buffer(0)]]) {
        float2 positions[3] = { float2(-1, -1), float2(3, -1), float2(-1, 3) };
        float2 p = positions[id];
        // Letterbox: keep the console's 256:240 pixel aspect on any screen.
        float picture = 256.0 / 240.0;
        float screen = viewport.x / max(viewport.y, 1.0);
        float2 scale = screen > picture
            ? float2(picture / screen, 1.0)
            : float2(1.0, screen / picture);
        VertexOut out;
        out.position = float4(p * scale, 0, 1);
        // Metal textures start at the top-left, clip space at the bottom-left.
        out.uv = float2((p.x + 1.0) * 0.5, 1.0 - (p.y + 1.0) * 0.5);
        return out;
    }

    fragment float4 nes_fragment(VertexOut in [[stage_in]],
                                 texture2d<float> picture [[texture(0)]],
                                 sampler nearest [[sampler(0)]]) {
        return picture.sample(nearest, in.uv);
    }
    """
}

extension MetalScreen: MTKViewDelegate {
    public func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {}

    public func draw(in view: MTKView) {
        guard
            let pipeline = makePipelineIfNeeded(),
            let sampler,
            let descriptor = view.currentRenderPassDescriptor,
            let drawable = view.currentDrawable,
            let buffer = commandQueue.makeCommandBuffer(),
            let encoder = buffer.makeRenderCommandEncoder(descriptor: descriptor)
        else {
            return
        }

        encoder.setRenderPipelineState(pipeline)
        encoder.setFragmentTexture(texture, index: 0)
        encoder.setFragmentSamplerState(sampler, index: 0)
        var viewport = SIMD2<Float>(Float(view.drawableSize.width), Float(view.drawableSize.height))
        encoder.setVertexBytes(&viewport, length: MemoryLayout<SIMD2<Float>>.size, index: 0)
        encoder.drawPrimitives(type: .triangle, vertexStart: 0, vertexCount: 3)
        encoder.endEncoding()

        buffer.present(drawable)
        buffer.commit()
    }
}
#endif
