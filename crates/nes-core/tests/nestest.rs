//! Nestest integration test
//!
//! This test runs the nestest.nes ROM to verify CPU implementation correctness.
//! Nestest is the gold standard CPU test ROM that tests every official and
//! unofficial opcode.
//!
//! Reference: https://www.nesdev.org/wiki/Nestest

use nes_core::cpu::Cpu;
use nes_core::mapper::create_mapper;
use nes_core::{Bus, JsnesError, Mapper};

/// Error code descriptions from nestest.txt, keyed by [byte, code].
/// Byte 0x02 errors (official opcodes):
const ERRORS_02: &[(u8, &str)] = &[
    (0x01, "BCS failed to branch"),
    (0x02, "BCS branched when it shouldn't have"),
    (0x03, "BCC branched when it shouldn't have"),
    (0x04, "BCC failed to branch"),
    (0x05, "BEQ failed to branch"),
    (0x06, "BEQ branched when it shouldn't have"),
    (0x07, "BNE failed to branch"),
    (0x08, "BNE branched when it shouldn't have"),
    (0x09, "BVS failed to branch"),
    (0x0a, "BVC branched when it shouldn't have"),
    (0x0b, "BVC failed to branch"),
    (0x0c, "BVS branched when it shouldn't have"),
    (0x0d, "BPL failed to branch"),
    (0x0e, "BPL branched when it shouldn't have"),
    (0x0f, "BMI failed to branch"),
    (0x10, "BMI branched when it shouldn't have"),
    (0x11, "PHP/flags failure (bits set)"),
    (0x12, "PHP/flags failure (bits clear)"),
    (0x13, "PHP/flags failure (misc bit states)"),
    (0x14, "PLP/flags failure (misc bit states)"),
    (0x15, "PLP/flags failure (misc bit states)"),
    (0x16, "PHA/PLA failure (PLA didn't affect Z and N properly)"),
    (0x17, "PHA/PLA failure (PLA didn't affect Z and N properly)"),
    (0x18, "ORA # failure"),
    (0x19, "ORA # failure"),
    (0x1a, "AND # failure"),
    (0x1b, "AND # failure"),
    (0x1c, "EOR # failure"),
    (0x1d, "EOR # failure"),
    (0x1e, "ADC # failure (overflow/carry problems)"),
    (0x1f, "ADC # failure (decimal mode was turned on)"),
    (0x20, "ADC # failure"),
    (0x21, "ADC # failure"),
    (0x22, "ADC # failure"),
    (0x23, "LDA # failure (didn't set N and Z correctly)"),
    (0x24, "LDA # failure (didn't set N and Z correctly)"),
    (0x25, "CMP # failure (messed up flags)"),
    (0x26, "CMP # failure (messed up flags)"),
    (0x27, "CMP # failure (messed up flags)"),
    (0x28, "CMP # failure (messed up flags)"),
    (0x29, "CMP # failure (messed up flags)"),
    (0x2a, "CMP # failure (messed up flags)"),
    (0x2b, "CPY # failure (messed up flags)"),
    (0x2c, "CPY # failure (messed up flags)"),
    (0x2d, "CPY # failure (messed up flags)"),
    (0x2e, "CPY # failure (messed up flags)"),
    (0x2f, "CPY # failure (messed up flags)"),
    (0x30, "CPY # failure (messed up flags)"),
    (0x31, "CPY # failure (messed up flags)"),
    (0x32, "CPX # failure (messed up flags)"),
    (0x33, "CPX # failure (messed up flags)"),
    (0x34, "CPX # failure (messed up flags)"),
    (0x35, "CPX # failure (messed up flags)"),
    (0x36, "CPX # failure (messed up flags)"),
    (0x37, "CPX # failure (messed up flags)"),
    (0x38, "CPX # failure (messed up flags)"),
    (0x39, "LDX # failure (didn't set N and Z correctly)"),
    (0x3a, "LDX # failure (didn't set N and Z correctly)"),
    (0x3b, "LDY # failure (didn't set N and Z correctly)"),
    (0x3c, "LDY # failure (didn't set N and Z correctly)"),
    (0x3d, "compare(s) stored the result in a register"),
    (0x3e, "INX/DEX/INY/DEY did something bad"),
    (0x3f, "INY/DEY messed up overflow or carry"),
    (0x40, "INX/DEX messed up overflow or carry"),
    (
        0x41,
        "TAY did something bad (changed wrong regs, messed up flags)",
    ),
    (
        0x42,
        "TAX did something bad (changed wrong regs, messed up flags)",
    ),
    (
        0x43,
        "TYA did something bad (changed wrong regs, messed up flags)",
    ),
    (
        0x44,
        "TXA did something bad (changed wrong regs, messed up flags)",
    ),
    (0x45, "TXS didn't set flags right, or TSX touched flags"),
    (
        0x46,
        "wrong data popped, or data not in right location on stack",
    ),
    (0x47, "JSR didn't work as expected"),
    (0x48, "RTS/JSR shouldn't have affected flags"),
    (
        0x49,
        "RTI/RTS didn't work right when return addys/data were manually pushed",
    ),
    (0x4a, "LSR A failed"),
    (0x4b, "ASL A failed"),
    (0x4c, "ROR A failed"),
    (0x4d, "ROL A failed"),
    (0x4e, "absolute,X NOPs less than 3 bytes long"),
    (0x4f, "implied NOPs affects regs/flags"),
    (0x50, "ZP,X NOPs less than 2 bytes long"),
    (0x51, "absolute NOP less than 3 bytes long"),
    (0x52, "ZP NOPs less than 2 bytes long"),
    (0x53, "absolute,X NOPs less than 3 bytes long"),
    (0x54, "implied NOPs affects regs/flags"),
    (0x55, "ZP,X NOPs less than 2 bytes long"),
    (0x56, "absolute NOP less than 3 bytes long"),
    (0x57, "ZP NOPs less than 2 bytes long"),
    (0x58, "(indirect,x) LDA didn't load expected data"),
    (
        0x59,
        "(indirect,x) STA didn't store data where it was supposed to",
    ),
    (0x5a, "(indirect,x) ORA failure"),
    (0x5b, "(indirect,x) ORA failure"),
    (0x5c, "(indirect,x) AND failure"),
    (0x5d, "(indirect,x) AND failure"),
    (0x5e, "(indirect,x) EOR failure"),
    (0x5f, "(indirect,x) EOR failure"),
    (0x60, "(indirect,x) ADC failure"),
    (0x61, "(indirect,x) ADC failure"),
    (0x62, "(indirect,x) ADC failure"),
    (0x63, "(indirect,x) ADC failure"),
    (0x64, "(indirect,x) ADC failure"),
    (0x65, "(indirect,x) CMP failure"),
    (0x66, "(indirect,x) CMP failure"),
    (0x67, "(indirect,x) CMP failure"),
    (0x68, "(indirect,x) CMP failure"),
    (0x69, "(indirect,x) CMP failure"),
    (0x6a, "(indirect,x) CMP failure"),
    (0x6b, "(indirect,x) CMP failure"),
    (0x6c, "(indirect,x) SBC failure"),
    (0x6d, "(indirect,x) SBC failure"),
    (0x6e, "(indirect,x) SBC failure"),
    (0x6f, "(indirect,x) SBC failure"),
    (0x70, "(indirect,x) SBC failure"),
    (0x71, "SBC # failure"),
    (0x72, "SBC # failure"),
    (0x73, "SBC # failure"),
    (0x74, "SBC # failure"),
    (0x75, "SBC # failure"),
    (0x76, "zeropage LDA didn't set the flags properly"),
    (0x77, "zeropage STA affected flags it shouldn't"),
    (0x78, "zeropage LDY didn't set the flags properly"),
    (0x79, "zeropage STY affected flags it shouldn't"),
    (0x7a, "zeropage LDX didn't set the flags properly"),
    (0x7b, "zeropage STX affected flags it shouldn't"),
    (0x7c, "zeropage BIT failure"),
    (0x7d, "zeropage BIT failure"),
    (0x7e, "zeropage ORA failure"),
    (0x7f, "zeropage ORA failure"),
    (0x80, "zeropage AND failure"),
    (0x81, "zeropage AND failure"),
    (0x82, "zeropage EOR failure"),
    (0x83, "zeropage EOR failure"),
    (0x84, "zeropage ADC failure"),
    (0x85, "zeropage ADC failure"),
    (0x86, "zeropage ADC failure"),
    (0x87, "zeropage ADC failure"),
    (0x88, "zeropage ADC failure"),
    (0x89, "zeropage CMP failure"),
    (0x8a, "zeropage CMP failure"),
    (0x8b, "zeropage CMP failure"),
    (0x8c, "zeropage CMP failure"),
    (0x8d, "zeropage CMP failure"),
    (0x8e, "zeropage CMP failure"),
    (0x8f, "zeropage CMP failure"),
    (0x90, "zeropage SBC failure"),
    (0x91, "zeropage SBC failure"),
    (0x92, "zeropage SBC failure"),
    (0x93, "zeropage SBC failure"),
    (0x94, "zeropage SBC failure"),
    (0x95, "zeropage CPX failure"),
    (0x96, "zeropage CPX failure"),
    (0x97, "zeropage CPX failure"),
    (0x98, "zeropage CPX failure"),
    (0x99, "zeropage CPX failure"),
    (0x9a, "zeropage CPX failure"),
    (0x9b, "zeropage CPX failure"),
    (0x9c, "zeropage CPY failure"),
    (0x9d, "zeropage CPY failure"),
    (0x9e, "zeropage CPY failure"),
    (0x9f, "zeropage CPY failure"),
    (0xa0, "zeropage CPY failure"),
    (0xa1, "zeropage CPY failure"),
    (0xa2, "zeropage CPY failure"),
    (0xa3, "zeropage LSR failure"),
    (0xa4, "zeropage LSR failure"),
    (0xa5, "zeropage ASL failure"),
    (0xa6, "zeropage ASL failure"),
    (0xa7, "zeropage ROL failure"),
    (0xa8, "zeropage ROL failure"),
    (0xa9, "zeropage ROR failure"),
    (0xaa, "zeropage ROR failure"),
    (0xab, "zeropage INC failure"),
    (0xac, "zeropage INC failure"),
    (0xad, "zeropage DEC failure"),
    (0xae, "zeropage DEC failure"),
    (0xaf, "zeropage DEC failure"),
    (0xb0, "absolute LDA didn't set the flags properly"),
    (0xb1, "absolute STA affected flags it shouldn't"),
    (0xb2, "absolute LDY didn't set the flags properly"),
    (0xb3, "absolute STY affected flags it shouldn't"),
    (0xb4, "absolute LDX didn't set the flags properly"),
    (0xb5, "absolute STX affected flags it shouldn't"),
    (0xb6, "absolute BIT failure"),
    (0xb7, "absolute BIT failure"),
    (0xb8, "absolute ORA failure"),
    (0xb9, "absolute ORA failure"),
    (0xba, "absolute AND failure"),
    (0xbb, "absolute AND failure"),
    (0xbc, "absolute EOR failure"),
    (0xbd, "absolute EOR failure"),
    (0xbe, "absolute ADC failure"),
    (0xbf, "absolute ADC failure"),
    (0xc0, "absolute ADC failure"),
    (0xc1, "absolute ADC failure"),
    (0xc2, "absolute ADC failure"),
    (0xc3, "absolute CMP failure"),
    (0xc4, "absolute CMP failure"),
    (0xc5, "absolute CMP failure"),
    (0xc6, "absolute CMP failure"),
    (0xc7, "absolute CMP failure"),
    (0xc8, "absolute CMP failure"),
    (0xc9, "absolute CMP failure"),
    (0xca, "absolute SBC failure"),
    (0xcb, "absolute SBC failure"),
    (0xcc, "absolute SBC failure"),
    (0xcd, "absolute SBC failure"),
    (0xce, "absolute SBC failure"),
    (0xcf, "absolute CPX failure"),
    (0xd0, "absolute CPX failure"),
    (0xd1, "absolute CPX failure"),
    (0xd2, "absolute CPX failure"),
    (0xd3, "absolute CPX failure"),
    (0xd4, "absolute CPX failure"),
    (0xd5, "absolute CPX failure"),
    (0xd6, "absolute CPY failure"),
    (0xd7, "absolute CPY failure"),
    (0xd8, "absolute CPY failure"),
    (0xd9, "absolute CPY failure"),
    (0xda, "absolute CPY failure"),
    (0xdb, "absolute CPY failure"),
    (0xdc, "absolute CPY failure"),
    (0xdd, "absolute LSR failure"),
    (0xde, "absolute LSR failure"),
    (0xdf, "absolute ASL failure"),
    (0xe0, "absolute ASL failure"),
    (0xe1, "absolute ROR failure"),
    (0xe2, "absolute ROR failure"),
    (0xe3, "absolute ROL failure"),
    (0xe4, "absolute ROL failure"),
    (0xe5, "absolute INC failure"),
    (0xe6, "absolute INC failure"),
    (0xe7, "absolute DEC failure"),
    (0xe8, "absolute DEC failure"),
    (0xe9, "absolute DEC failure"),
    (0xea, "(indirect),y LDA didn't load what it was supposed to"),
    (
        0xeb,
        "(indirect),y read location should've wrapped around 0xFFFF to 0x0000",
    ),
    (0xec, "(indirect),y should've wrapped zeropage address"),
    (0xed, "(indirect),y ORA failure"),
    (0xee, "(indirect),y ORA failure"),
    (0xef, "(indirect),y AND failure"),
    (0xf0, "(indirect),y AND failure"),
    (0xf1, "(indirect),y EOR failure"),
    (0xf2, "(indirect),y EOR failure"),
    (0xf3, "(indirect),y ADC failure"),
    (0xf4, "(indirect),y ADC failure"),
    (0xf5, "(indirect),y ADC failure"),
    (0xf6, "(indirect),y ADC failure"),
    (0xf7, "(indirect),y ADC failure"),
    (0xf8, "(indirect),y CMP failure"),
    (0xf9, "(indirect),y CMP failure"),
    (0xfa, "(indirect),y CMP failure"),
    (0xfb, "(indirect),y CMP failure"),
    (0xfc, "(indirect),y CMP failure"),
    (0xfd, "(indirect),y CMP failure"),
    (0xfe, "(indirect),y CMP failure"),
];

/// Byte 0x03 errors (unofficial opcodes and addressing modes):
const ERRORS_03: &[(u8, &str)] = &[
    (0x01, "(indirect),y SBC failure"),
    (0x02, "(indirect),y SBC failure"),
    (0x03, "(indirect),y SBC failure"),
    (0x04, "(indirect),y SBC failure"),
    (0x05, "(indirect),y SBC failure"),
    (0x06, "(indirect),y STA failure"),
    (
        0x07,
        "JMP () data reading didn't wrap properly (fails on 65C02)",
    ),
    (0x08, "zeropage,x LDY,X failure"),
    (0x09, "zeropage,x LDY,X failure"),
    (0x0a, "zeropage,x STY,X failure"),
    (0x0b, "zeropage,x ORA failure"),
    (0x0c, "zeropage,x ORA failure"),
    (0x0d, "zeropage,x AND failure"),
    (0x0e, "zeropage,x AND failure"),
    (0x0f, "zeropage,x EOR failure"),
    (0x10, "zeropage,x EOR failure"),
    (0x11, "zeropage,x ADC failure"),
    (0x12, "zeropage,x ADC failure"),
    (0x13, "zeropage,x ADC failure"),
    (0x14, "zeropage,x ADC failure"),
    (0x15, "zeropage,x ADC failure"),
    (0x16, "zeropage,x CMP failure"),
    (0x17, "zeropage,x CMP failure"),
    (0x18, "zeropage,x CMP failure"),
    (0x19, "zeropage,x CMP failure"),
    (0x1a, "zeropage,x CMP failure"),
    (0x1b, "zeropage,x CMP failure"),
    (0x1c, "zeropage,x CMP failure"),
    (0x1d, "zeropage,x SBC failure"),
    (0x1e, "zeropage,x SBC failure"),
    (0x1f, "zeropage,x SBC failure"),
    (0x20, "zeropage,x SBC failure"),
    (0x21, "zeropage,x SBC failure"),
    (0x22, "zeropage,x LDA failure"),
    (0x23, "zeropage,x LDA failure"),
    (0x24, "zeropage,x STA failure"),
    (0x25, "zeropage,x LSR failure"),
    (0x26, "zeropage,x LSR failure"),
    (0x27, "zeropage,x ASL failure"),
    (0x28, "zeropage,x ASL failure"),
    (0x29, "zeropage,x ROR failure"),
    (0x2a, "zeropage,x ROR failure"),
    (0x2b, "zeropage,x ROL failure"),
    (0x2c, "zeropage,x ROL failure"),
    (0x2d, "zeropage,x INC failure"),
    (0x2e, "zeropage,x INC failure"),
    (0x2f, "zeropage,x DEC failure"),
    (0x30, "zeropage,x DEC failure"),
    (0x31, "zeropage,x DEC failure"),
    (0x32, "zeropage,x LDX,Y failure"),
    (0x33, "zeropage,x LDX,Y failure"),
    (0x34, "zeropage,x STX,Y failure"),
    (0x35, "zeropage,x STX,Y failure"),
    (0x36, "absolute,y LDA failure"),
    (
        0x37,
        "absolute,y LDA failure to wrap properly from 0xFFFF to 0x0000",
    ),
    (0x38, "absolute,y LDA failure, page cross"),
    (0x39, "absolute,y ORA failure"),
    (0x3a, "absolute,y ORA failure"),
    (0x3b, "absolute,y AND failure"),
    (0x3c, "absolute,y AND failure"),
    (0x3d, "absolute,y EOR failure"),
    (0x3e, "absolute,y EOR failure"),
    (0x3f, "absolute,y ADC failure"),
    (0x40, "absolute,y ADC failure"),
    (0x41, "absolute,y ADC failure"),
    (0x42, "absolute,y ADC failure"),
    (0x43, "absolute,y ADC failure"),
    (0x44, "absolute,y CMP failure"),
    (0x45, "absolute,y CMP failure"),
    (0x46, "absolute,y CMP failure"),
    (0x47, "absolute,y CMP failure"),
    (0x48, "absolute,y CMP failure"),
    (0x49, "absolute,y CMP failure"),
    (0x4a, "absolute,y CMP failure"),
    (0x4b, "absolute,y SBC failure"),
    (0x4c, "absolute,y SBC failure"),
    (0x4d, "absolute,y SBC failure"),
    (0x4e, "absolute,y SBC failure"),
    (0x4f, "absolute,y SBC failure"),
    (0x50, "absolute,y STA failure"),
    (0x51, "absolute,x LDY,X failure"),
    (0x52, "absolute,x LDY,X failure (didn't page cross)"),
    (0x53, "absolute,x ORA failure"),
    (0x54, "absolute,x ORA failure"),
    (0x55, "absolute,x AND failure"),
    (0x56, "absolute,x AND failure"),
    (0x57, "absolute,x EOR failure"),
    (0x58, "absolute,x EOR failure"),
    (0x59, "absolute,x ADC failure"),
    (0x5a, "absolute,x ADC failure"),
    (0x5b, "absolute,x ADC failure"),
    (0x5c, "absolute,x ADC failure"),
    (0x5d, "absolute,x ADC failure"),
    (0x5e, "absolute,x CMP failure"),
    (0x5f, "absolute,x CMP failure"),
    (0x60, "absolute,x CMP failure"),
    (0x61, "absolute,x CMP failure"),
    (0x62, "absolute,x CMP failure"),
    (0x63, "absolute,x CMP failure"),
    (0x64, "absolute,x CMP failure"),
    (0x65, "absolute,x CMP failure"),
    (0x66, "absolute,x SBC failure"),
    (0x67, "absolute,x SBC failure"),
    (0x68, "absolute,x SBC failure"),
    (0x69, "absolute,x SBC failure"),
    (0x6a, "absolute,x SBC failure"),
    (0x6b, "absolute,x LDA failure"),
    (0x6c, "absolute,x LDA failure (didn't page cross)"),
    (0x6d, "absolute,x STA failure"),
    (0x6e, "absolute,x LSR failure"),
    (0x6f, "absolute,x LSR failure"),
    (0x70, "absolute,x ASL failure"),
    (0x71, "absolute,x ASL failure"),
    (0x72, "absolute,x ROR failure"),
    (0x73, "absolute,x ROR failure"),
    (0x74, "absolute,x ROL failure"),
    (0x75, "absolute,x ROL failure"),
    (0x76, "absolute,x INC failure"),
    (0x77, "absolute,x INC failure"),
    (0x78, "absolute,x DEC failure"),
    (0x79, "absolute,x DEC failure"),
    (0x7a, "absolute,x DEC failure"),
    (0x7b, "absolute,x LDX,Y failure"),
    (0x7c, "absolute,x LDX,Y failure"),
    (0x7d, "LAX (indr,x) failure"),
    (0x7e, "LAX (indr,x) failure"),
    (0x7f, "LAX zeropage failure"),
    (0x80, "LAX zeropage failure"),
    (0x81, "LAX absolute failure"),
    (0x82, "LAX absolute failure"),
    (0x83, "LAX (indr),y failure"),
    (0x84, "LAX (indr),y failure"),
    (0x85, "LAX zp,y failure"),
    (0x86, "LAX zp,y failure"),
    (0x87, "LAX abs,y failure"),
    (0x88, "LAX abs,y failure"),
    (0x89, "SAX (indr,x) failure"),
    (0x8a, "SAX (indr,x) failure"),
    (0x8b, "SAX zeropage failure"),
    (0x8c, "SAX zeropage failure"),
    (0x8d, "SAX absolute failure"),
    (0x8e, "SAX absolute failure"),
    (0x8f, "SAX zp,y failure"),
    (0x90, "SAX zp,y failure"),
    (0x91, "SBC (unofficial) failure"),
    (0x92, "SBC (unofficial) failure"),
    (0x93, "SBC (unofficial) failure"),
    (0x94, "SBC (unofficial) failure"),
    (0x95, "SBC (unofficial) failure"),
    (0x96, "DCP (indr,x) failure"),
    (0x97, "DCP (indr,x) failure"),
    (0x98, "DCP (indr,x) failure"),
    (0x99, "DCP zeropage failure"),
    (0x9a, "DCP zeropage failure"),
    (0x9b, "DCP zeropage failure"),
    (0x9c, "DCP absolute failure"),
    (0x9d, "DCP absolute failure"),
    (0x9e, "DCP absolute failure"),
    (0x9f, "DCP (indr),y failure"),
    (0xa0, "DCP (indr),y failure"),
    (0xa1, "DCP (indr),y failure"),
    (0xa2, "DCP zp,x failure"),
    (0xa3, "DCP zp,x failure"),
    (0xa4, "DCP zp,x failure"),
    (0xa5, "DCP abs,y failure"),
    (0xa6, "DCP abs,y failure"),
    (0xa7, "DCP abs,y failure"),
    (0xa8, "DCP abs,x failure"),
    (0xa9, "DCP abs,x failure"),
    (0xaa, "DCP abs,x failure"),
    (0xab, "ISC (indr,x) failure"),
    (0xac, "ISC (indr,x) failure"),
    (0xad, "ISC (indr,x) failure"),
    (0xae, "ISC zeropage failure"),
    (0xaf, "ISC zeropage failure"),
    (0xb0, "ISC zeropage failure"),
    (0xb1, "ISC absolute failure"),
    (0xb2, "ISC absolute failure"),
    (0xb3, "ISC absolute failure"),
    (0xb4, "ISC (indr),y failure"),
    (0xb5, "ISC (indr),y failure"),
    (0xb6, "ISC (indr),y failure"),
    (0xb7, "ISC zp,x failure"),
    (0xb8, "ISC zp,x failure"),
    (0xb9, "ISC zp,x failure"),
    (0xba, "ISC abs,y failure"),
    (0xbb, "ISC abs,y failure"),
    (0xbc, "ISC abs,y failure"),
    (0xbd, "ISC abs,x failure"),
    (0xbe, "ISC abs,x failure"),
    (0xbf, "ISC abs,x failure"),
    (0xc0, "SLO (indr,x) failure"),
    (0xc1, "SLO (indr,x) failure"),
    (0xc2, "SLO (indr,x) failure"),
    (0xc3, "SLO zeropage failure"),
    (0xc4, "SLO zeropage failure"),
    (0xc5, "SLO zeropage failure"),
    (0xc6, "SLO absolute failure"),
    (0xc7, "SLO absolute failure"),
    (0xc8, "SLO absolute failure"),
    (0xc9, "SLO (indr),y failure"),
    (0xca, "SLO (indr),y failure"),
    (0xcb, "SLO (indr),y failure"),
    (0xcc, "SLO zp,x failure"),
    (0xcd, "SLO zp,x failure"),
    (0xce, "SLO zp,x failure"),
    (0xcf, "SLO abs,y failure"),
    (0xd0, "SLO abs,y failure"),
    (0xd1, "SLO abs,y failure"),
    (0xd2, "SLO abs,x failure"),
    (0xd3, "SLO abs,x failure"),
    (0xd4, "SLO abs,x failure"),
    (0xd5, "RLA (indr,x) failure"),
    (0xd6, "RLA (indr,x) failure"),
    (0xd7, "RLA (indr,x) failure"),
    (0xd8, "RLA zeropage failure"),
    (0xd9, "RLA zeropage failure"),
    (0xda, "RLA zeropage failure"),
    (0xdb, "RLA absolute failure"),
    (0xdc, "RLA absolute failure"),
    (0xdd, "RLA absolute failure"),
    (0xde, "RLA (indr),y failure"),
    (0xdf, "RLA (indr),y failure"),
    (0xe0, "RLA (indr),y failure"),
    (0xe1, "RLA zp,x failure"),
    (0xe2, "RLA zp,x failure"),
    (0xe3, "RLA zp,x failure"),
    (0xe4, "RLA abs,y failure"),
    (0xe5, "RLA abs,y failure"),
    (0xe6, "RLA abs,y failure"),
    (0xe7, "RLA abs,x failure"),
    (0xe8, "RLA abs,x failure"),
    (0xe9, "RLA abs,x failure"),
    (0xea, "SRE (indr,x) failure"),
    (0xeb, "SRE (indr,x) failure"),
    (0xec, "SRE zeropage failure"),
    (0xed, "SRE zeropage failure"),
    (0xee, "SRE zeropage failure"),
    (0xef, "SRE absolute failure"),
    (0xf0, "SRE absolute failure"),
    (0xf1, "SRE absolute failure"),
    (0xf2, "SRE (indr),y failure"),
    (0xf3, "SRE (indr),y failure"),
    (0xf4, "SRE (indr),y failure"),
    (0xf5, "SRE zp,x failure"),
    (0xf6, "SRE zp,x failure"),
    (0xf7, "SRE zp,x failure"),
    (0xf8, "SRE abs,y failure"),
    (0xf9, "SRE abs,y failure"),
    (0xfa, "SRE abs,y failure"),
    (0xfb, "SRE abs,x failure"),
    (0xfc, "SRE abs,x failure"),
    (0xfd, "SRE abs,x failure"),
];

/// Test groups for byte 0x02 (official opcodes)
const TEST_GROUPS_02: &[(&str, u8, u8)] = &[
    ("Branch tests", 0x01, 0x10),
    ("Flag tests (PHP/PLP/PHA/PLA)", 0x11, 0x17),
    ("Immediate instruction tests", 0x18, 0x3d),
    ("Implied instruction tests (INX/DEX/TAX/etc)", 0x3e, 0x45),
    ("Stack tests", 0x46, 0x49),
    ("Accumulator shift tests (LSR/ASL/ROR/ROL A)", 0x4a, 0x4d),
    ("Unofficial NOP tests", 0x4e, 0x57),
    ("(indirect,x) tests", 0x58, 0x70),
    ("SBC # tests", 0x71, 0x75),
    ("Zeropage tests", 0x76, 0xaf),
    ("Absolute tests", 0xb0, 0xe9),
    ("(indirect),y tests", 0xea, 0xfe),
];

/// Test groups for byte 0x03 (unofficial opcodes)
const TEST_GROUPS_03: &[(&str, u8, u8)] = &[
    ("(indirect),y tests (continued)", 0x01, 0x06),
    ("JMP indirect wrapping test", 0x07, 0x07),
    ("Zeropage,x tests", 0x08, 0x31),
    ("Zeropage,x LDX,Y / STX,Y tests", 0x32, 0x35),
    ("Absolute,y tests", 0x36, 0x50),
    ("Absolute,x tests", 0x51, 0x7b),
    ("Unofficial LAX tests", 0x7c, 0x87),
    ("Unofficial SAX tests", 0x88, 0x8f),
    ("Unofficial SBC tests", 0x90, 0x94),
    ("Unofficial DCP tests", 0x95, 0xa9),
    ("Unofficial ISC tests", 0xaa, 0xbe),
    ("Unofficial SLO tests", 0xbf, 0xd3),
    ("Unofficial RLA tests", 0xd4, 0xe8),
    ("Unofficial SRE tests", 0xe9, 0xfd),
];

/// iNES ROM header structure
#[derive(Debug)]
struct RomHeader {
    prg_rom_size: usize, // in 16KB units
    chr_rom_size: usize, // in 8KB units
    mapper_number: u16,
    has_trainer: bool,
}

impl RomHeader {
    fn parse(data: &[u8]) -> Result<(Self, usize), JsnesError> {
        if data.len() < 16 {
            return Err(JsnesError::InvalidRomFormat(
                "ROM too small for header".to_string(),
            ));
        }

        // Check iNES magic number
        if &data[0..4] != b"NES\x1a" {
            return Err(JsnesError::InvalidRomFormat(
                "Invalid iNES magic number".to_string(),
            ));
        }

        let prg_rom_size = data[4] as usize * 16384; // 16KB units
        let chr_rom_size = data[5] as usize * 8192; // 8KB units
        let flags6 = data[6];
        let flags7 = data[7];

        let has_trainer = (flags6 & 0x04) != 0;

        // Mapper number: lower 4 bits from flags6, upper 4 bits from flags7
        let mapper_number = ((flags7 & 0xF0) | ((flags6 & 0xF0) >> 4)) as u16;

        let header = RomHeader {
            prg_rom_size,
            chr_rom_size,
            mapper_number,
            has_trainer,
        };

        let header_size = 16;
        let trainer_size = if has_trainer { 512 } else { 0 };
        let data_offset = header_size + trainer_size;

        Ok((header, data_offset))
    }
}

/// Simple Bus implementation for testing
struct TestBus {
    cpu_ram: [u8; 0x800],
    mapper: Box<dyn Mapper>,
    nmi_pending: bool,
    irq_pending: bool,
}

impl TestBus {
    fn new(mapper: Box<dyn Mapper>) -> Self {
        Self {
            cpu_ram: [0; 0x800],
            mapper,
            nmi_pending: false,
            irq_pending: false,
        }
    }
}

impl Bus for TestBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // CPU internal RAM ($0000-$07FF, mirrored to $1FFF)
            0x0000..=0x1FFF => self.cpu_ram[(addr & 0x07FF) as usize],
            // PPU registers ($2000-$3FFF, mirrored)
            0x2000..=0x3FFF => {
                // PPU not implemented for nestest - return open bus (0)
                0
            }
            // APU and I/O registers ($4000-$401F)
            0x4000..=0x401F => {
                // APU not implemented for nestest - return open bus (0)
                0
            }
            // Cartridge space ($4020-$FFFF)
            _ => self.mapper.read(addr),
        }
    }

    fn write(&mut self, addr: u16, value: u8) {
        match addr {
            // CPU internal RAM
            0x0000..=0x1FFF => {
                self.cpu_ram[(addr & 0x07FF) as usize] = value;
            }
            // PPU registers
            0x2000..=0x3FFF => {
                // PPU not implemented for nestest
            }
            // APU and I/O registers
            0x4000..=0x401F => {
                // APU not implemented for nestest
            }
            // Cartridge space
            _ => self.mapper.write(addr, value),
        }
    }

    fn on_cpu_cycle(&mut self, _cycles: u8) {
        // No PPU/APU to update in this simple test
    }

    fn nmi_pending(&self) -> bool {
        self.nmi_pending
    }

    fn clear_nmi(&mut self) {
        self.nmi_pending = false;
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending || self.mapper.irq_pending()
    }
}

/// Load a ROM file from the given path
fn load_rom(path: &str) -> Result<(RomHeader, Vec<u8>, Vec<u8>), JsnesError> {
    let data = std::fs::read(path).map_err(|e| JsnesError::IoError(e.to_string()))?;

    let (header, data_offset) = RomHeader::parse(&data)?;

    // Extract PRG-ROM
    let prg_start = data_offset;
    let prg_end = prg_start + header.prg_rom_size;
    if data.len() < prg_end {
        return Err(JsnesError::InvalidRomFormat(
            "ROM too small for PRG-ROM".to_string(),
        ));
    }
    let prg_rom = data[prg_start..prg_end].to_vec();

    // Extract CHR-ROM
    let chr_start = prg_end;
    let chr_end = chr_start + header.chr_rom_size;
    let chr_rom = if data.len() >= chr_end {
        data[chr_start..chr_end].to_vec()
    } else {
        vec![] // CHR-RAM will be created by mapper
    };

    Ok((header, prg_rom, chr_rom))
}

/// Get error description for a result byte
fn describe_error(byte: &str, code: u8, error_table: &[(u8, &str)]) -> String {
    if code == 0 {
        return format!("byte {} = 0x00 (all tests passed)", byte);
    }

    for &(err_code, desc) in error_table {
        if err_code == code {
            return format!("byte {} error 0x{:02X}: {}", byte, code, desc);
        }
    }

    format!("byte {} error 0x{:02X} (unknown error code)", byte, code)
}

/// Run nestest in automation mode
///
/// The automation entry point at 0xC000 bypasses the interactive menu
/// and runs all tests sequentially. Results are stored in memory:
///   mem[0x02] = page 1 result (official opcodes)
///   mem[0x03] = page 2 result (unofficial opcodes)
/// A value of 0x00 means all tests in that page passed.
fn run_nestest(rom_path: &str) -> Result<NestestResults, JsnesError> {
    // Load the ROM
    let (header, prg_rom, chr_rom) = load_rom(rom_path)?;

    // Create the mapper
    let mapper = create_mapper(header.mapper_number, prg_rom, chr_rom)?;

    // Create the bus and CPU
    let mut bus = TestBus::new(mapper);
    let mut cpu = Cpu::new();

    // Initialize CPU for nestest automation mode
    // Start execution at $C000 (not the standard reset vector)
    // The nestest ROM expects to start at $C000 for automation

    // First, do a normal reset to initialize the CPU state
    cpu.reset(&mut bus);

    // Then override PC to $C000 for automation mode
    // Note: The CPU's PC will be incremented before fetching the first instruction,
    // so we need to set it to $C000 - 1
    // Actually, looking at the execute_instruction implementation, PC is advanced
    // during operand fetch, so we should set PC to $C000 directly

    // Access the private fields via a workaround - we need to add methods to Cpu
    // For now, let's use a different approach: manually step through

    // Actually, let's check if we can set PC through a method
    // Looking at cpu.rs, we have pc() getter but no setter
    // We need to add a method to set PC for testing

    // For now, let's manually execute from the reset vector which should point to $C000
    // in the nestest ROM

    // Run instructions until we hit a stopping condition
    let max_instructions = 100_000;
    let mut instruction_count = 0;
    let mut crashed = false;
    let mut crash_message = String::new();

    // Set PC to $C000 for automation mode
    // This bypasses the interactive menu and runs all tests sequentially
    cpu.set_pc(0xC000);

    while instruction_count < max_instructions {
        // Fetch opcode at current PC
        let opcode = bus.read(cpu.pc());

        // Check for KIL opcodes (0x02, 0x12, 0x22, 0x32, 0x42, 0x52, 0x62, 0x72, 0x92, 0xB2, 0xD2, 0xF2)
        // These are "unofficial" opcodes that halt the CPU
        if matches!(
            opcode,
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2
        ) {
            // Check if we're in the open bus region ($4018-$5FFF)
            // A crash in open bus is expected when open bus is properly emulated
            let pc = cpu.pc();
            if pc >= 0x4018 && pc <= 0x5FFF {
                // Expected crash in open bus region
                break;
            } else {
                crashed = true;
                crash_message = format!("KIL opcode at ${:04X}", pc);
                break;
            }
        }

        // Execute the instruction
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cpu.execute_instruction(&mut bus, opcode);
        })) {
            Ok(_) => {}
            Err(_) => {
                crashed = true;
                crash_message = format!("Panic at ${:04X}", cpu.pc());
                break;
            }
        }

        instruction_count += 1;

        // Check if we've reached a stable state (test completed)
        // The ROM stores results at $02 and $03
        // If both are non-zero or we've run many instructions, we can check
        if instruction_count > 50000 {
            // Check results periodically after many instructions
            let result02 = bus.read(0x0002);
            let result03 = bus.read(0x0003);

            // If both results are set and stable, we can stop
            // But we need to run long enough for all tests to complete
        }
    }

    // Read final results
    let result02 = bus.read(0x0002);
    let result03 = bus.read(0x0003);

    Ok(NestestResults {
        result02,
        result03,
        instruction_count,
        crashed,
        crash_message,
    })
}

/// Results from running nestest
#[derive(Debug)]
struct NestestResults {
    result02: u8,
    result03: u8,
    instruction_count: usize,
    crashed: bool,
    crash_message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn get_nestest_rom_path() -> PathBuf {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
        let mut path = PathBuf::from(manifest_dir);
        // CARGO_MANIFEST_DIR is <repo>/crates/nes-core; climb back to <repo>.
        path.pop();
        path.pop();
        path.push("roms");
        path.push("nestest");
        path.push("nestest.nes");
        path
    }

    #[test]
    fn test_nestest_automation() {
        let rom_path = get_nestest_rom_path();

        // Check if ROM file exists
        if !rom_path.exists() {
            eprintln!("Warning: nestest.nes not found at {}", rom_path.display());
            eprintln!("Skipping nestest integration test");
            return;
        }

        let rom_path_str = rom_path.to_str().expect("Invalid path");

        let results = run_nestest(rom_path_str).expect("Failed to run nestest");

        // Print summary
        println!("\n");
        println!(
            "nestest results: byte 0x02 = 0x{:02X}, byte 0x03 = 0x{:02X}",
            results.result02, results.result03
        );
        println!("Instructions executed: {}", results.instruction_count);

        if results.crashed {
            println!("ROM crashed: {}", results.crash_message);
        }

        // Check that we ran enough instructions
        assert!(
            results.instruction_count > 1000,
            "Test ROM didn't run enough instructions (only {})",
            results.instruction_count
        );

        if results.crashed {
            println!(
                "ROM crashed after test completion (expected): {}",
                results.crash_message
            );
        }

        // Print detailed results
        if results.result02 == 0 && results.result03 == 0 {
            println!("All tests passed!");
        } else {
            if results.result02 != 0 {
                println!(
                    "Last failure (page 1): {}",
                    describe_error("0x02", results.result02, ERRORS_02)
                );
            }
            if results.result03 != 0 {
                println!(
                    "Last failure (page 2): {}",
                    describe_error("0x03", results.result03, ERRORS_03)
                );
            }
        }

        // Assert all tests passed
        assert_eq!(
            results.result02,
            0,
            "Official opcode tests failed: {}",
            describe_error("0x02", results.result02, ERRORS_02)
        );

        assert_eq!(
            results.result03,
            0,
            "Unofficial opcode tests failed: {}",
            describe_error("0x03", results.result03, ERRORS_03)
        );
    }

    /// Test individual test groups for byte 0x02
    #[test]
    fn test_nestest_official_opcodes() {
        let rom_path = get_nestest_rom_path();
        if !rom_path.exists() {
            return;
        }

        let rom_path_str = rom_path.to_str().expect("Invalid path");
        let results = run_nestest(rom_path_str).expect("Failed to run nestest");

        // Test each group
        for (name, from, to) in TEST_GROUPS_02 {
            if results.result02 >= *from && results.result02 <= *to {
                panic!(
                    "{} failed: {}",
                    name,
                    describe_error("0x02", results.result02, ERRORS_02)
                );
            }
        }
    }

    /// Test individual test groups for byte 0x03
    #[test]
    fn test_nestest_unofficial_opcodes() {
        let rom_path = get_nestest_rom_path();
        if !rom_path.exists() {
            return;
        }

        let rom_path_str = rom_path.to_str().expect("Invalid path");
        let results = run_nestest(rom_path_str).expect("Failed to run nestest");

        // Test each group
        for (name, from, to) in TEST_GROUPS_03 {
            if results.result03 >= *from && results.result03 <= *to {
                panic!(
                    "{} failed: {}",
                    name,
                    describe_error("0x03", results.result03, ERRORS_03)
                );
            }
        }
    }
}
