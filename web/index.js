/**
 * JSNES WASM Web Frontend
 * 
 * A simple web interface for the JSNES WASM emulator.
 * Handles canvas rendering, Web Audio API, keyboard input, and ROM loading.
 */

// Import the WASM module
// The WASM files should be in nes-rust/crates/nes-wasm/pkg/
import init, { 
  JsNes, 
  get_button_a, 
  get_button_b, 
  get_button_up, 
  get_button_down, 
  get_button_left, 
  get_button_right, 
  get_button_start, 
  get_button_select 
} from './pkg/nes_wasm.js';

// NES display dimensions
const NES_WIDTH = 256;
const NES_HEIGHT = 240;
const SCALE = 2;

// Button constants
const BUTTONS = {
  A: null,
  B: null,
  UP: null,
  DOWN: null,
  LEFT: null,
  RIGHT: null,
  START: null,
  SELECT: null
};

// Keyboard to button mapping
const KEY_MAP = {
  'ArrowUp': 'UP',
  'ArrowDown': 'DOWN',
  'ArrowLeft': 'LEFT',
  'ArrowRight': 'RIGHT',
  'KeyX': 'A',
  'KeyZ': 'B',
  'Enter': 'START',
  'ShiftLeft': 'SELECT',
  'ShiftRight': 'SELECT'
};

// Global state
let nes = null;
let canvas = null;
let ctx = null;
let imageData = null;
let audioContext = null;
let audioBuffer = null;
let audioSource = null;
let isRunning = false;
let isPaused = false;
let animationId = null;

/**
 * Initialize button constants from WASM
 */
function initButtonConstants() {
  BUTTONS.A = get_button_a();
  BUTTONS.B = get_button_b();
  BUTTONS.UP = get_button_up();
  BUTTONS.DOWN = get_button_down();
  BUTTONS.LEFT = get_button_left();
  BUTTONS.RIGHT = get_button_right();
  BUTTONS.START = get_button_start();
  BUTTONS.SELECT = get_button_select();
}

/**
 * Update status message
 */
function setStatus(message, type = 'loading') {
  const status = document.getElementById('status');
  status.textContent = message;
  status.className = type;
}

/**
 * Initialize the canvas for rendering
 */
function initCanvas() {
  canvas = document.getElementById('nes-canvas');
  ctx = canvas.getContext('2d', { alpha: false });
  
  // Create ImageData for direct pixel manipulation
  // The frame buffer is 256x240 pixels
  imageData = ctx.createImageData(NES_WIDTH, NES_HEIGHT);
  
  // Disable smoothing for pixel-perfect rendering
  ctx.imageSmoothingEnabled = false;
}

/**
 * Initialize Web Audio API
 */
function initAudio() {
  try {
    audioContext = new (window.AudioContext || window.webkitAudioContext)({
      sampleRate: 48000
    });
    
    // Create a script processor for audio output
    // Note: ScriptProcessorNode is deprecated but still widely supported
    // AudioWorklet would be better but requires more setup
    const bufferSize = 4096;
    audioBuffer = audioContext.createScriptProcessor(bufferSize, 0, 2);
    
    audioBuffer.onaudioprocess = (e) => {
      if (!nes || isPaused) {
        // Output silence
        const left = e.outputBuffer.getChannelData(0);
        const right = e.outputBuffer.getChannelData(1);
        left.fill(0);
        right.fill(0);
        return;
      }
      
      // Get audio samples from the emulator
      const samples = nes.get_audio_samples();
      const left = e.outputBuffer.getChannelData(0);
      const right = e.outputBuffer.getChannelData(1);
      
      // Copy samples to output buffers
      // Samples are interleaved: [left, right, left, right, ...]
      for (let i = 0, j = 0; i < left.length && j < samples.length; i++, j += 2) {
        left[i] = samples[j] || 0;
        right[i] = samples[j + 1] || 0;
      }
    };
    
    audioBuffer.connect(audioContext.destination);
    
    return true;
  } catch (e) {
    console.error('Failed to initialize audio:', e);
    return false;
  }
}

/**
 * Render the current frame to the canvas
 */
function renderFrame() {
  if (!nes) return;
  
  // Get the frame buffer from the emulator
  // Returns Uint32Array of ARGB pixels (0xAARRGGBB format)
  const frameBuffer = nes.get_frame_buffer();
  
  // Convert ARGB to RGBA for canvas ImageData
  const data = imageData.data;
  for (let i = 0, j = 0; i < frameBuffer.length; i++, j += 4) {
    const pixel = frameBuffer[i];
    // ARGB format: 0xAARRGGBB
    data[j] = (pixel >> 16) & 0xFF;     // R
    data[j + 1] = (pixel >> 8) & 0xFF;  // G
    data[j + 2] = pixel & 0xFF;         // B
    data[j + 3] = (pixel >> 24) & 0xFF; // A
  }
  
  // Draw to canvas with scaling
  // Create a temporary canvas for the unscaled image
  const tempCanvas = document.createElement('canvas');
  tempCanvas.width = NES_WIDTH;
  tempCanvas.height = NES_HEIGHT;
  const tempCtx = tempCanvas.getContext('2d');
  tempCtx.putImageData(imageData, 0, 0);
  
  // Draw scaled version
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.drawImage(tempCanvas, 0, 0, canvas.width, canvas.height);
}

/**
 * Main game loop
 */
function gameLoop() {
  if (!isRunning) return;
  
  if (!isPaused && nes) {
    // Execute one frame of emulation
    nes.frame();
    
    // Render the frame
    renderFrame();
  }
  
  // Schedule next frame
  animationId = requestAnimationFrame(gameLoop);
}

/**
 * Start the emulator
 */
function startEmulator() {
  if (!isRunning) {
    isRunning = true;
    isPaused = false;
    gameLoop();
    
    // Resume audio context if suspended
    if (audioContext && audioContext.state === 'suspended') {
      audioContext.resume();
    }
    
    document.getElementById('pause-btn').textContent = 'Pause';
    document.getElementById('pause-btn').disabled = false;
    document.getElementById('reset-btn').disabled = false;
  }
}

/**
 * Stop the emulator
 */
function stopEmulator() {
  isRunning = false;
  isPaused = false;
  if (animationId) {
    cancelAnimationFrame(animationId);
    animationId = null;
  }
}

/**
 * Pause/resume the emulator
 */
function togglePause() {
  isPaused = !isPaused;
  document.getElementById('pause-btn').textContent = isPaused ? 'Resume' : 'Pause';
  
  if (!isPaused && audioContext && audioContext.state === 'suspended') {
    audioContext.resume();
  }
}

/**
 * Reset the emulator
 */
function resetEmulator() {
  if (nes) {
    nes.reset();
    setStatus('Emulator reset', 'success');
  }
}

/**
 * Load a ROM file
 */
async function loadROM(file) {
  if (!file) return;
  
  try {
    setStatus('Loading ROM...');
    
    // Read the file as ArrayBuffer
    const arrayBuffer = await file.arrayBuffer();
    const uint8Array = new Uint8Array(arrayBuffer);
    
    // Stop current emulation
    stopEmulator();
    
    // Create new NES instance
    if (nes) {
      nes.free();
    }
    nes = new JsNes();
    
    // Load the ROM
    nes.load_rom(uint8Array);
    
    setStatus(`Loaded: ${file.name}`, 'success');
    
    // Start emulation
    startEmulator();
    
  } catch (error) {
    console.error('Failed to load ROM:', error);
    setStatus(`Error: ${error.message}`, 'error');
  }
}

/**
 * Handle keyboard events
 */
function handleKeyDown(e) {
  if (!nes) return;
  
  const buttonName = KEY_MAP[e.code];
  if (buttonName) {
    e.preventDefault();
    const button = BUTTONS[buttonName];
    nes.button_down(1, button);
  }
}

function handleKeyUp(e) {
  if (!nes) return;
  
  const buttonName = KEY_MAP[e.code];
  if (buttonName) {
    e.preventDefault();
    const button = BUTTONS[buttonName];
    nes.button_up(1, button);
  }
}

/**
 * Setup event listeners
 */
function setupEventListeners() {
  // File input
  document.getElementById('rom-file').addEventListener('change', (e) => {
    const file = e.target.files[0];
    if (file) {
      loadROM(file);
    }
  });
  
  // Reset button
  document.getElementById('reset-btn').addEventListener('click', resetEmulator);
  
  // Pause button
  document.getElementById('pause-btn').addEventListener('click', togglePause);
  
  // Keyboard controls
  window.addEventListener('keydown', handleKeyDown);
  window.addEventListener('keyup', handleKeyUp);
  
  // Drag and drop
  const gameContainer = document.getElementById('game-container');
  
  gameContainer.addEventListener('dragover', (e) => {
    e.preventDefault();
    gameContainer.style.borderColor = '#e94560';
  });
  
  gameContainer.addEventListener('dragleave', () => {
    gameContainer.style.borderColor = '#16213e';
  });
  
  gameContainer.addEventListener('drop', (e) => {
    e.preventDefault();
    gameContainer.style.borderColor = '#16213e';
    
    const file = e.dataTransfer.files[0];
    if (file && file.name.endsWith('.nes')) {
      loadROM(file);
    } else {
      setStatus('Please drop a .nes file', 'error');
    }
  });
}

/**
 * Main initialization
 */
async function main() {
  try {
    // Initialize the WASM module
    setStatus('Loading WASM module...');
    await init();
    
    // Initialize button constants
    initButtonConstants();
    
    // Initialize canvas
    initCanvas();
    
    // Initialize audio
    const audioInitialized = initAudio();
    if (!audioInitialized) {
      setStatus('Audio initialization failed, continuing without sound...', 'error');
    }
    
    // Setup event listeners
    setupEventListeners();
    
    setStatus('Ready! Load a ROM file to start.', 'success');
    
  } catch (error) {
    console.error('Initialization failed:', error);
    setStatus(`Initialization failed: ${error.message}`, 'error');
  }
}

// Start the application
main();
