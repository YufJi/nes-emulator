// Simple test to verify the WASM module works
import init, { JsNes } from './pkg/nes_wasm.js';

async function test() {
    console.log("Starting test...");
    await init();
    console.log("Init done");
    
    const nes = new JsNes();
    console.log("Nes created");
    
    // Create a minimal test ROM (iNES header + minimal data)
    // This is not a valid NES ROM, just for testing load_rom doesn't crash
    const testRom = new Uint8Array([
        0x4E, 0x45, 0x53, 0x1A, // NES magic
        0x01, // 16KB PRG
        0x01, // 8KB CHR  
        0x00, // flags6
        0x00, // flags7 (mapper 0)
        0x00, 0x00, 0x00, 0x00, // padding
        0x00, 0x00, 0x00, 0x00,
        // 16KB PRG ROM
        ...new Array(16384).fill(0xEA), // NOP instructions
        // 8KB CHR ROM
        ...new Array(8192).fill(0x00),
    ]);
    
    console.log("Test ROM size:", testRom.length);
    
    try {
        nes.load_rom(testRom);
        console.log("ROM loaded!");
        
        nes.frame();
        console.log("Frame executed!");
        
        const fb = nes.get_frame_buffer();
        console.log("Frame buffer size:", fb.length);
        console.log("Test PASSED!");
    } catch (e) {
        console.error("Test FAILED:", e);
    }
}

test();
