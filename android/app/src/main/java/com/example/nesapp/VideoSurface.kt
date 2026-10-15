package com.example.nesapp

import android.content.Context
import android.opengl.GLES20
import android.opengl.GLSurfaceView
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.FloatBuffer
import javax.microedition.khronos.egl.EGLConfig
import javax.microedition.khronos.opengles.GL10
import uniffi.nes_uniffi.frameHeight
import uniffi.nes_uniffi.frameWidth

/**
 * The picture: a texture upload driven by the console's own frame buffer.
 *
 * `RENDERMODE_WHEN_DIRTY` on purpose. The emulation thread knows when a new
 * picture exists, so the GL thread is woken with `requestRender()` instead of
 * re-uploading the same frame at the display's pace — which also means a slow
 * frame is drawn once rather than twice, and a 50 Hz PAL cartridge is not
 * forced through a 60 Hz redraw loop.
 *
 * The view is one instance for the life of the app: android's `GLSurfaceView`
 * recreates its EGL context on re-attach, so leaving a game and starting another
 * reuses it rather than leaking contexts.
 */
class VideoSurface(context: Context, private val settings: Settings) : GLSurfaceView(context) {

    private val renderer = FrameRenderer(settings)

    init {
        setEGLContextClientVersion(2)
        setRenderer(renderer)
        renderMode = RENDERMODE_WHEN_DIRTY
    }

    /** Hand the newest picture over; the GL thread picks it up on the next draw. */
    fun show(frame: ByteArray) {
        renderer.pending = frame
    }

    /** Drop the picture, so a new game does not start on the last one's last frame. */
    fun clearPicture() {
        renderer.pending = null
        renderer.discard()
        requestRender()
    }
}

/**
 * The GLES2 side of [VideoSurface].
 *
 * Everything is one texture and one quad: the console's picture is already
 * exactly the pixels that belong on screen, so there is nothing to rasterise,
 * only to place — by whole pixels where the screen allows it.
 */
private class FrameRenderer(private val settings: Settings) : GLSurfaceView.Renderer {

    /** The newest frame from the emulation thread; the GL thread takes it. */
    @Volatile
    var pending: ByteArray? = null

    private var program = 0
    private var texture = 0
    private var positionAttrib = 0
    private var uvAttrib = 0
    private var textureUniform = 0
    private var scanlinesUniform = 0

    private var surfaceWidth = 0
    private var surfaceHeight = 0

    /** Whether the texture has been allocated at its full size yet. */
    private var allocated = false

    private val width = frameWidth().toInt()
    private val height = frameHeight().toInt()

    /** A quad: two triangles as a strip, with the picture's top at v = 0. */
    private val vertices: FloatBuffer = ByteBuffer
        .allocateDirect(4 * 4 * Float.SIZE_BYTES)
        .order(ByteOrder.nativeOrder())
        .asFloatBuffer()
        .apply {
            put(
                floatArrayOf(
                    -1f, -1f, 0f, 1f,
                    1f, -1f, 1f, 1f,
                    -1f, 1f, 0f, 0f,
                    1f, 1f, 1f, 0f,
                ),
            )
            position(0)
        }

    /** Forget the texture, for when the EGL context has been thrown away. */
    fun discard() {
        allocated = false
        texture = 0
    }

    override fun onSurfaceCreated(unused: GL10, config: EGLConfig) {
        // A new context means every GL object is gone; rebuild them all.
        discard()
        program = buildProgram()
        positionAttrib = GLES20.glGetAttribLocation(program, "aPosition")
        uvAttrib = GLES20.glGetAttribLocation(program, "aTexCoord")
        textureUniform = GLES20.glGetUniformLocation(program, "uTexture")
        scanlinesUniform = GLES20.glGetUniformLocation(program, "uScanlines")

        val textures = IntArray(1)
        GLES20.glGenTextures(1, textures, 0)
        texture = textures[0]
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, texture)
        // Nearest, both ways: the console's pixels are the pixels to show, and
        // smoothing them is exactly the blur the hardware never had.
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MIN_FILTER, GLES20.GL_NEAREST)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_MAG_FILTER, GLES20.GL_NEAREST)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_S, GLES20.GL_CLAMP_TO_EDGE)
        GLES20.glTexParameteri(GLES20.GL_TEXTURE_2D, GLES20.GL_TEXTURE_WRAP_T, GLES20.GL_CLAMP_TO_EDGE)
        // Allocate once, then upload with `glTexSubImage2D`, which does not
        // reallocate storage per frame.
        GLES20.glTexImage2D(
            GLES20.GL_TEXTURE_2D,
            0,
            GLES20.GL_RGBA,
            width,
            height,
            0,
            GLES20.GL_RGBA,
            GLES20.GL_UNSIGNED_BYTE,
            null,
        )
        allocated = true

        GLES20.glDisable(GLES20.GL_DEPTH_TEST)
        GLES20.glDisable(GLES20.GL_BLEND)
    }

    override fun onSurfaceChanged(unused: GL10, width: Int, height: Int) {
        surfaceWidth = width
        surfaceHeight = height
    }

    override fun onDrawFrame(unused: GL10) {
        val frame = pending
        if (frame != null && frame.size == width * height * PIXEL_BYTES) {
            pending = null
            upload(frame)
        }

        GLES20.glClearColor(0f, 0f, 0f, 1f)
        GLES20.glClear(GLES20.GL_COLOR_BUFFER_BIT)

        val (viewportWidth, viewportHeight) = pictureViewport(
            surfaceWidth = surfaceWidth,
            surfaceHeight = surfaceHeight,
            pictureWidth = width,
            pictureHeight = height,
            stretch = !settings.integerScale,
        )
        GLES20.glViewport(
            (surfaceWidth - viewportWidth) / 2,
            (surfaceHeight - viewportHeight) / 2,
            viewportWidth,
            viewportHeight,
        )

        GLES20.glUseProgram(program)

        vertices.position(0)
        GLES20.glEnableVertexAttribArray(positionAttrib)
        GLES20.glVertexAttribPointer(positionAttrib, 2, GLES20.GL_FLOAT, false, 4 * Float.SIZE_BYTES, vertices)

        vertices.position(2)
        GLES20.glEnableVertexAttribArray(uvAttrib)
        GLES20.glVertexAttribPointer(uvAttrib, 2, GLES20.GL_FLOAT, false, 4 * Float.SIZE_BYTES, vertices)

        GLES20.glActiveTexture(GLES20.GL_TEXTURE0)
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, texture)
        GLES20.glUniform1i(textureUniform, 0)

        GLES20.glUniform1f(
            scanlinesUniform,
            scanlineRows(viewportHeight, height, settings.crtFilter),
        )

        GLES20.glDrawArrays(GLES20.GL_TRIANGLE_STRIP, 0, 4)
        GLES20.glDisableVertexAttribArray(positionAttrib)
        GLES20.glDisableVertexAttribArray(uvAttrib)
    }

    private fun upload(frame: ByteArray) {
        if (!allocated) return
        GLES20.glBindTexture(GLES20.GL_TEXTURE_2D, texture)
        GLES20.glTexSubImage2D(
            GLES20.GL_TEXTURE_2D,
            0,
            0,
            0,
            width,
            height,
            GLES20.GL_RGBA,
            GLES20.GL_UNSIGNED_BYTE,
            ByteBuffer.wrap(frame),
        )
    }

    private fun buildProgram(): Int {
        val vertex = compile(GLES20.GL_VERTEX_SHADER, VERTEX_SHADER)
        val fragment = compile(GLES20.GL_FRAGMENT_SHADER, FRAGMENT_SHADER)
        val shaderProgram = GLES20.glCreateProgram()
        GLES20.glAttachShader(shaderProgram, vertex)
        GLES20.glAttachShader(shaderProgram, fragment)
        GLES20.glLinkProgram(shaderProgram)
        val status = IntArray(1)
        GLES20.glGetProgramiv(shaderProgram, GLES20.GL_LINK_STATUS, status, 0)
        check(status[0] == GLES20.GL_TRUE) { "program link failed: ${GLES20.glGetProgramInfoLog(shaderProgram)}" }
        // The shaders themselves are no longer needed once linked.
        GLES20.glDeleteShader(vertex)
        GLES20.glDeleteShader(fragment)
        return shaderProgram
    }

    private fun compile(type: Int, source: String): Int {
        val shader = GLES20.glCreateShader(type)
        GLES20.glShaderSource(shader, source)
        GLES20.glCompileShader(shader)
        val status = IntArray(1)
        GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, status, 0)
        check(status[0] == GLES20.GL_TRUE) { "shader compile failed: ${GLES20.glGetShaderInfoLog(shader)}" }
        return shader
    }

    private companion object {
        const val PIXEL_BYTES = 4

        const val VERTEX_SHADER = """
            attribute vec2 aPosition;
            attribute vec2 aTexCoord;
            varying vec2 vTexCoord;
            void main() {
                vTexCoord = aTexCoord;
                gl_Position = vec4(aPosition, 0.0, 1.0);
            }
        """

        /**
         * The picture, with the optional CRT treatment.
         *
         * `.bgr` is not decoration: the binding hands over `0xAARRGGBB` as
         * little-endian bytes, so a pixel arrives as `B, G, R, A`. `GL_RGBA`
         * expects red first, so the channels are swapped here rather than in a
         * second copy on the CPU (or with `GL_EXT_texture_format_BGRA8888`,
         * which not every TV box advertises).
         */
        const val FRAGMENT_SHADER = """
            precision mediump float;
            varying vec2 vTexCoord;
            uniform sampler2D uTexture;
            uniform float uScanlines;
            void main() {
                vec3 pixel = texture2D(uTexture, vTexCoord).bgr;
                float shade = 1.0;
                if (uScanlines > 0.0) {
                    float row = floor(gl_FragCoord.y / uScanlines);
                    shade = mod(row, 2.0) < 1.0 ? 0.72 : 1.0;
                    // A gentle corner falloff, the other half of what makes a
                    // tube look like a tube.
                    vec2 offset = vTexCoord - 0.5;
                    shade *= 1.0 - 0.55 * dot(offset, offset);
                }
                gl_FragColor = vec4(pixel * shade, 1.0);
            }
        """
    }
}
