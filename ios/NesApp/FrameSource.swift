import Foundation

/// A zero-copy view of the console's current frame.
///
/// The pixels are the core's own buffer, not a copy of it: on a little-endian
/// host `0xAARRGGBB` lands in memory as `B, G, R, A`, which is exactly
/// `MTLPixelFormatBGRA8Unorm`. Hand `pixels` to
/// `MTLBuffer(bytesNoCopy:length:options:deallocator:)` (or a `CGDataProvider`)
/// and the picture never touches the CPU twice.
///
/// The buffer belongs to the console, so consume it before emulating the next
/// frame — do not keep the view, or a texture built from it, for later.
public struct FrameSource {
    /// Frame width in pixels; read it from the console rather than assuming 256.
    public let width: Int
    /// Frame height in pixels.
    public let height: Int
    /// The pixels, `height` rows of `width` little-endian `UInt32`.
    public let pixels: UnsafeRawBufferPointer

    /// Bytes in one row, for `MTLTexture.replace(region:...)`.
    public var bytesPerRow: Int { width * 4 }

    /// Bytes in the whole picture.
    public var byteCount: Int { pixels.count }
}
