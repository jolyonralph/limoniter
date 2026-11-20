; ch14_rastertime.s - how long things take, shown the classic way: each routine changes the background colour
; (COLOR00) as it starts, so it draws a band as tall as the time it takes. Ten routines run every frame, one
; after another: five ways for the processor to clear 1536 bytes of chip RAM, one more clearing a buffer in any
; memory, the blitter clearing the chip RAM, and three ways to multiply 256 numbers by 40. The panel on the
; right names each band's colour and gives its time in raster lines, measured from the beam's position, says
; which processor it ran on, and where the program and the "any memory" buffer were loaded. Left mouse button
; to leave.
;
; How To Code v8, chapter 14. CC0: use it for anything.
;
; Run it on an Amiga 500 and then on something faster: the bands change, and not all in the same way. It's made
; for PAL; on NTSC the last bands run off the bottom of the display.
;
; $VER: ch14_rastertime 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8
PLANEBYTES      equ     ROWBYTES*HEIGHT
FIRSTLINE       equ     $2c             ; the first line of the display: the measuring starts here
LINECLOCKS      equ     227             ; colour clocks in a PAL line (a 68000 at 7.09 MHz runs two cycles in each)

CLEARBYTES      equ     1536            ; what each clear clears: a multiple of 4, of 64 and of 48
MOVEMBYTES      equ     48              ; what one MOVEM of 12 registers writes
COUNT           equ     256             ; how many numbers are multiplied

PANELCOLUMN     equ     20              ; the panel starts 20 bytes (160 pixels) across
TEXTCOLUMN      equ     21
NAMECOLUMN      equ     23
TIMECOLUMN      equ     35
LEGENDTOP       equ     60              ; the first legend row, in pixels from the top
LEGENDSTEP      equ     12

; A bit of AttnFlags beyond the ones system.i names (from the NDK 3.2's exec/execbase.i).
AFB_FPGA        equ     10              ; an FPGA processor (the NDK 3.2 names it; older includes don't)

; Each test in the table: its code, its name, its colour, and its time (colour clocks) once measured.
TEST_CODE       equ     0
TEST_NAME       equ     4
TEST_COLOUR     equ     8
TEST_TIME       equ     10
TEST_SIZE       equ     14

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the screen, the tables, and the generated code.

Init:
        ; The three bitplanes' addresses into the copper list.
        move.l  #Plane1,d0
        lea     CopperBitplanes,a0
        moveq   #3-1,d1
.plane:
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        add.l   #PLANEBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane

        ; All eight sprites at an empty sprite, so the mouse pointer doesn't show with the system running.
        move.l  #NoSprite,d0
        lea     CopperSprites,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
.pal:

        ; The bitplanes are in a BSS section, which the system clears as it loads the program. The panel is
        ; bitplane 2, the right half of every line.
        lea     Plane2+PANELCOLUMN,a0
        move.w  #HEIGHT-1,d1
        moveq   #-1,d2
.panel:
        moveq   #(ROWBYTES-PANELCOLUMN)/4-1,d0
.panelrow:
        move.l  d2,(a0)+
        dbf     d0,.panelrow
        lea     PANELCOLUMN(a0),a0
        dbf     d1,.panel

        ; A ruler down the left edge in bitplane 1: a tick every 10 lines, a longer one every 50, counted from
        ; the first line, where the bands start.
        lea     Plane1,a0
        moveq   #HEIGHT/10,d1           ; 26 ticks: lines 0, 10, ... 250
        moveq   #0,d2                   ; counts to five: every fifth tick is a long one
.tick:
        move.b  #$f0,(a0)
        tst.w   d2
        bne.s   .short
        move.b  #$ff,(a0)
.short:
        addq.w  #1,d2
        cmp.w   #5,d2
        bne.s   .next
        moveq   #0,d2
.next:
        lea     ROWBYTES*10(a0),a0
        dbf     d1,.tick

        ; The colour swatches, in bitplane 3 inside the panel: there they show colour 6, which the copper list
        ; sets to each test's colour just above its row.
        lea     Plane3+LEGENDTOP*ROWBYTES+TEXTCOLUMN,a0
        lea     CopperSwatches,a1
        lea     Tests,a2
.swatch:
        tst.l   TEST_CODE(a2)
        beq.s   .swatchesdone
        moveq   #7-1,d0
        move.l  a0,a3
.swatchrow:
        move.b  #$7e,(a3)
        lea     ROWBYTES(a3),a3
        dbf     d0,.swatchrow
        move.w  TEST_COLOUR(a2),6(a1)
        addq.l  #8,a1
        lea     ROWBYTES*LEGENDSTEP(a0),a0
        lea     TEST_SIZE(a2),a2
        bra.s   .swatch
.swatchesdone:

        ; The names, and the text that doesn't change.
        lea     Tests,a4
        moveq   #LEGENDTOP,d4
.name:
        tst.l   TEST_CODE(a4)
        beq.s   .namesdone
        move.l  TEST_NAME(a4),a0
        moveq   #NAMECOLUMN,d0
        move.w  d4,d1
        bsr     DrawText
        add.w   #LEGENDSTEP,d4
        lea     TEST_SIZE(a4),a4
        bra.s   .name
.namesdone:
        lea     StaticText,a4
.static:
        move.w  (a4)+,d1
        bmi.s   .staticdone
        moveq   #TEXTCOLUMN,d0
        move.l  a4,a0
        bsr     DrawText
        move.l  a0,a4                   ; DrawText leaves a0 after the string's zero: on to the next
        move.l  a4,d0
        addq.l  #1,d0
        and.w   #-2,d0                  ; the next entry starts on an even address
        move.l  d0,a4
        bra.s   .static
.staticdone:

        ; Which processor, from AttnFlags.
        bsr     WhichProcessor
        moveq   #TEXTCOLUMN+4,d0
        moveq   #18,d1
        bsr     DrawText
        move.l  ExecBase.w,a6
        btst    #AFB_FPGA-8,AttnFlags(a6)
        beq.s   .notfpga
        lea     FpgaText,a0
        moveq   #TEXTCOLUMN+10,d0
        moveq   #18,d1
        bsr     DrawText
.notfpga:

        ; Where things are: the VBR (from the startup), this code, and the buffer in "any" memory. On an Amiga
        ; 500 with a trapdoor expansion the code is at $C0xxxx, in slow RAM; with fast RAM, it's there.
        move.l  VectorBase,d0
        moveq   #TEXTCOLUMN+5,d1
        moveq   #28,d2
        bsr     DrawHex
        move.l  #TestClrLoop,d0
        moveq   #TEXTCOLUMN+5,d1
        moveq   #38,d2
        bsr     DrawHex
        move.l  #ClearAny,d0
        moveq   #TEXTCOLUMN+5,d1
        moveq   #48,d2
        bsr     DrawHex

        ; The numbers to multiply, and the table of the answers for the table lookup.
        lea     Numbers,a0
        lea     Times40,a1
        moveq   #0,d0
        moveq   #0,d1
        move.w  #COUNT-1,d2
.number:
        move.w  d0,(a0)+
        move.w  d1,(a1)+
        addq.w  #1,d0
        add.w   #40,d1
        dbf     d2,.number

        bsr     MakeCode
        rts

; ---------------------------------------------------------------------------------------------------------------
; Writes the fully unrolled clear: 32 copies of the MOVEM in TestMovemLoop, then an RTS. Code written as data
; may be sitting in the data cache, and the instruction cache may hold whatever was there before; so after
; writing it, the caches are cleared. CacheClearU does it properly from Kickstart 2.04 (V37) on; before that
; there's no call for it, and a 68020 or 68030 has its instruction cache cleared here by hand. (Kickstart 1.3
; doesn't switch on a 68030's data cache, and doesn't run on a 68040.)

MakeCode:
        lea     GeneratedCode,a0
        move.l  MovemInstruction,d0     ; the instruction, as the assembler made it: two words
        moveq   #CLEARBYTES/MOVEMBYTES-1,d1
.copy:
        move.l  d0,(a0)+
        dbf     d1,.copy
        move.w  #$4e75,(a0)             ; rts

        move.l  ExecBase.w,a6
        cmp.w   #37,LIB_VERSION(a6)
        blo.s   .old
        jsr     _LVOCacheClearU(a6)
        lea     FlushedClearU,a0
        bra.s   .say
.old:
        lea     FlushedNone,a0
        btst    #AFB_68020,AttnFlags+1(a6)
        beq.s   .say
        lea     ClearInstructionCache(pc),a5
        jsr     _LVOSupervisor(a6)
        lea     FlushedCacr,a0
.say:
        moveq   #TEXTCOLUMN,d0
        move.w  #LEGENDTOP+LEGENDSTEP*10+8,d1
        bsr     DrawText
        rts

; Run in supervisor mode by Supervisor(), on a 68020 or 68030 only: sets the "clear instruction cache" bit in
; the CACR, keeping the rest. Written as words, as for the 68000 there's no MOVEC.

ClearInstructionCache:
        dc.l    $4e7a0002               ; movec cacr,d0
        or.w    #$0008,d0               ; CACR bit 3: clear the instruction cache
        dc.l    $4e7b0002               ; movec d0,cacr
        rte

; ---------------------------------------------------------------------------------------------------------------
; Which processor this is, from exec's AttnFlags: a0 = its name. Each bit is set for that processor and every
; later one, so the highest bit set wins. Kickstart 1.3 knows only the 68010 and 68020 bits (a 68030 shows as a
; 68020); bit 7, the 68060, is set by the 68060.library, not by Kickstart 3.1's exec.

WhichProcessor:
        move.l  ExecBase.w,a6
        move.w  AttnFlags(a6),d0
        lea     Name68060,a0
        btst    #AFB_68060,d0
        bne.s   .known
        lea     Name68040,a0
        btst    #AFB_68040,d0
        bne.s   .known
        lea     Name68030,a0
        btst    #AFB_68030,d0
        bne.s   .known
        lea     Name68020,a0
        btst    #AFB_68020,d0
        bne.s   .known
        lea     Name68010,a0
        btst    #AFB_68010,d0
        bne.s   .known
        lea     Name68000,a0
.known:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: wait for the first line of the display, then run each test with its own background colour,
; noting the beam's position before and after. Then the times go into the panel, while the bottom of the screen
; is drawn: the drawing isn't measured.

Frame:
        move.w  #FIRSTLINE,d0
        bsr     WaitLine
        lea     Tests,a6                ; a6 is kept by every test
.test:
        move.l  TEST_CODE(a6),d0
        beq.s   .tested
        move.w  TEST_COLOUR(a6),COLOR00(a5)
        bsr     BeamPosition
        move.l  d0,TEST_TIME(a6)
        move.l  TEST_CODE(a6),a0
        jsr     (a0)
        lea     CUSTOM,a5
        bsr     BeamPosition
        sub.l   TEST_TIME(a6),d0
        move.l  d0,TEST_TIME(a6)
        lea     TEST_SIZE(a6),a6
        bra.s   .test
.tested:
        move.w  #$000,COLOR00(a5)

        lea     Tests,a4
        moveq   #LEGENDTOP,d4
.time:
        tst.l   TEST_CODE(a4)
        beq.s   .done
        move.l  TEST_TIME(a4),d0
        lea     TimeText,a0
        bsr     FormatLines
        lea     TimeText,a0
        moveq   #TIMECOLUMN,d0
        move.w  d4,d1
        bsr     DrawText
        add.w   #LEGENDSTEP,d4
        lea     TEST_SIZE(a4),a4
        bra.s   .time
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The tests. Each may change any register but a6 (and the stack); a5 is CUSTOM as each starts.

; 384 times CLR.L (A0)+ and DBF: 20 + 10 cycles each on a 68000. CLR reads the memory before it writes it.
TestClrLoop:
        lea     ClearChip,a0
        move.w  #CLEARBYTES/4-1,d0
.loop:
        clr.l   (a0)+
        dbf     d0,.loop
        rts

; 384 times MOVE.L D1,(A0)+ and DBF: 12 + 10 cycles.
TestMoveLoop:
        lea     ClearChip,a0
        moveq   #0,d1
        move.w  #CLEARBYTES/4-1,d0
.loop:
        move.l  d1,(a0)+
        dbf     d0,.loop
        rts

; The same, 16 to a loop: the DBF's 10 cycles are spent once for 64 bytes, not once for 4.
TestMoveUnrolled:
        lea     ClearChip,a0
        moveq   #0,d1
        moveq   #CLEARBYTES/64-1,d0
.loop:
        rept    16
        move.l  d1,(a0)+
        endr
        dbf     d0,.loop
        rts

; MOVEM: 12 registers of zero written backwards from the end, 48 bytes in 8 + 8*12 = 104 cycles.
TestMovemLoop:
        lea     ClearChip+CLEARBYTES,a0
        bsr     ZeroRegisters
        moveq   #CLEARBYTES/MOVEMBYTES-1,d0
MovemInstruction:
        movem.l d1-d7/a1-a5,-(a0)       ; MakeCode copies this instruction
        dbf     d0,MovemInstruction
        rts

; The same MOVEM with no loop: 32 of them, written by MakeCode as the program starts.
TestMovemCode:
        lea     ClearChip+CLEARBYTES,a0
        bsr     ZeroRegisters
        jmp     GeneratedCode           ; it ends with an RTS, which returns from here

; The MOVEM loop again, clearing a buffer in any memory: fast RAM if there is some.
TestMovemAny:
        lea     ClearAny+CLEARBYTES,a0
        bsr     ZeroRegisters
        moveq   #CLEARBYTES/MOVEMBYTES-1,d0
.loop:
        movem.l d1-d7/a1-a5,-(a0)
        dbf     d0,.loop
        rts

; The blitter: D only, minterm 0, 24 rows of 32 words. The processor waits for it (BLIT_END).
TestBlitter:
        BLIT_BEGIN
        move.l  #ClearChip,BLTDPTH(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  #$01000000,BLTCON0(a5)  ; BLTCON0 = $0100 (D on, minterm 0); BLTCON1 = 0
        move.w  #((CLEARBYTES/64)<<6)!32,BLTSIZE(a5)
        BLIT_END
        rts

; Each number times 40 with MULU: 38 + 2 cycles for each 1 bit in the source (40 has two), + 4 for the #40.
TestMulu:
        lea     Numbers,a0
        lea     Products,a1
        move.w  #COUNT-1,d1
.loop:
        move.w  (a0)+,d0
        mulu    #40,d0
        move.w  d0,(a1)+
        dbf     d1,.loop
        rts

; Times 40 as (n*4 + n) * 8: three ADDs and a shift.
TestShiftAdd:
        lea     Numbers,a0
        lea     Products,a1
        move.w  #COUNT-1,d1
.loop:
        move.w  (a0)+,d0
        move.w  d0,d2
        add.w   d0,d0                   ; n*2
        add.w   d0,d0                   ; n*4
        add.w   d2,d0                   ; n*5
        lsl.w   #3,d0                   ; n*40
        move.w  d0,(a1)+
        dbf     d1,.loop
        rts

; Times 40 from a table made once in Init.
TestTable:
        lea     Numbers,a0
        lea     Products,a1
        lea     Times40,a2
        move.w  #COUNT-1,d1
.loop:
        move.w  (a0)+,d0
        add.w   d0,d0                   ; a word for each number
        move.w  (a2,d0.w),(a1)+
        dbf     d1,.loop
        rts

; Twelve registers of zero, for the MOVEMs: d1-d7 and a1-a5.
ZeroRegisters:
        moveq   #0,d1
        moveq   #0,d2
        moveq   #0,d3
        moveq   #0,d4
        moveq   #0,d5
        moveq   #0,d6
        moveq   #0,d7
        move.l  d1,a1
        move.l  d1,a2
        move.l  d1,a3
        move.l  d1,a4
        move.l  d1,a5
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits for a line (d0, 0 to 311) to start. a5 = CUSTOM.

WaitLine:
.wait:
        move.l  VPOSR(a5),d1
        lsr.l   #8,d1
        and.w   #$1ff,d1
        cmp.w   d0,d1
        bne.s   .wait
        rts

; Where the beam is, in colour clocks from the top of the frame: d0 = line * 227 + position on the line.
; VPOSR read as a long has the line's top bit in bit 16, the rest of it in bits 15-8, and the position (in colour
; clocks) in bits 7-0.

BeamPosition:
        move.l  VPOSR(a5),d0
        moveq   #0,d1
        move.b  d0,d1
        lsr.l   #8,d0
        and.w   #$1ff,d0
        mulu    #LINECLOCKS,d0
        add.l   d1,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Text, 8 by 8 pixels a character, into bitplane 1. a0 = the string (ending in zero; capitals, figures and some
; signs), d0 = the column (0 to 39), d1 = the row, in pixels. Returns a0 just after the string's zero.

DrawText:
        lea     Plane1,a1
        mulu    #ROWBYTES,d1
        add.l   d1,a1
        add.w   d0,a1
.char:
        moveq   #0,d0
        move.b  (a0)+,d0
        beq.s   .done
        sub.b   #32,d0
        and.w   #63,d0                  ; only the 64 characters from the space on
        lsl.w   #3,d0
        lea     Font,a2
        add.w   d0,a2
        move.b  (a2)+,(a1)
        move.b  (a2)+,ROWBYTES*1(a1)
        move.b  (a2)+,ROWBYTES*2(a1)
        move.b  (a2)+,ROWBYTES*3(a1)
        move.b  (a2)+,ROWBYTES*4(a1)
        move.b  (a2)+,ROWBYTES*5(a1)
        move.b  (a2)+,ROWBYTES*6(a1)
        move.b  (a2)+,ROWBYTES*7(a1)
        addq.l  #1,a1
        bra.s   .char
.done:
        rts

; A time in colour clocks (d0) as raster lines to a tenth, "nnn.n", at a0.
FormatLines:
        move.l  d0,d1                   ; times 10 (it can be more than a word holds, so not MULU)
        lsl.l   #3,d0
        add.l   d1,d1
        add.l   d1,d0
        add.l   #LINECLOCKS/2,d0        ; rounded to the nearest tenth
        divu    #LINECLOCKS,d0          ; tenths of a line
        and.l   #$ffff,d0
        divu    #10,d0                  ; lines, and the tenths left over in the top word
        swap    d0
        add.b   #"0",d0
        move.b  d0,4(a0)
        move.b  #".",3(a0)
        clr.b   5(a0)
        swap    d0
        lea     3(a0),a1
        moveq   #3-1,d2
.digit:
        and.l   #$ffff,d0
        divu    #10,d0
        swap    d0
        add.b   #"0",d0
        move.b  d0,-(a1)
        swap    d0
        dbf     d2,.digit
        cmp.b   #"0",(a0)               ; no noughts in front
        bne.s   .done
        move.b  #" ",(a0)
        cmp.b   #"0",1(a0)
        bne.s   .done
        move.b  #" ",1(a0)
.done:
        rts

; d0 as "$" and eight hex digits, at column d1 and row d2.
DrawHex:
        lea     HexText,a0
        move.b  #"$",(a0)+
        moveq   #8-1,d3
.digit:
        rol.l   #4,d0
        move.w  d0,d4
        and.w   #$f,d4
        move.b  HexDigits(pc,d4.w),(a0)+
        dbf     d3,.digit
        clr.b   (a0)
        lea     HexText,a0
        move.w  d1,d0
        move.w  d2,d1
        bra     DrawText

HexDigits:
        dc.b    "0123456789ABCDEF"

; ---------------------------------------------------------------------------------------------------------------

VBlank:
        rts

        section data,data

; The tests, in the order they run: code, name, colour (and room for the time).
Tests:
        dc.l    TestClrLoop,NameClrLoop
        dc.w    $c22
        dc.l    0
        dc.l    TestMoveLoop,NameMoveLoop
        dc.w    $e80
        dc.l    0
        dc.l    TestMoveUnrolled,NameMoveUnrolled
        dc.w    $dd2
        dc.l    0
        dc.l    TestMovemLoop,NameMovemLoop
        dc.w    $4c4
        dc.l    0
        dc.l    TestMovemCode,NameMovemCode
        dc.w    $1a9
        dc.l    0
        dc.l    TestMovemAny,NameMovemAny
        dc.w    $4bf
        dc.l    0
        dc.l    TestBlitter,NameBlitter
        dc.w    $66f
        dc.l    0
        dc.l    TestMulu,NameMulu
        dc.w    $b5e
        dc.l    0
        dc.l    TestShiftAdd,NameShiftAdd
        dc.w    $f7a
        dc.l    0
        dc.l    TestTable,NameTable
        dc.w    $999
        dc.l    0
        dc.l    0                       ; the end

NameClrLoop:      dc.b  "CLR.L LOOP",0
NameMoveLoop:     dc.b  "MOVE.L LOOP",0
NameMoveUnrolled: dc.b  "MOVE.L X16",0
NameMovemLoop:    dc.b  "MOVEM LOOP",0
NameMovemCode:    dc.b  "MOVEM X32",0
NameMovemAny:     dc.b  "MOVEM ANY",0
NameBlitter:      dc.b  "BLITTER",0
NameMulu:         dc.b  "MULU #40",0
NameShiftAdd:     dc.b  "ADD+SHIFT",0
NameTable:        dc.b  "TABLE",0

Name68000:      dc.b    "68000",0
Name68010:      dc.b    "68010",0
Name68020:      dc.b    "68020",0
Name68030:      dc.b    "68030",0
Name68040:      dc.b    "68040",0
Name68060:      dc.b    "68060",0
FpgaText:       dc.b    "FPGA",0
FlushedClearU:  dc.b    "FLUSH CACHECLEARU",0
FlushedCacr:    dc.b    "FLUSH MOVEC CACR",0
FlushedNone:    dc.b    "FLUSH NOT NEEDED",0
        even

; The text that doesn't change: its row, then the text. Each entry starts on an even address.
StaticText:
        dc.w    6
        dc.b    "RASTER TIME",0
        even
        dc.w    18
        dc.b    "CPU",0
        even
        dc.w    28
        dc.b    "VBR",0
        even
        dc.w    38
        dc.b    "CODE",0
        even
        dc.w    48
        dc.b    "ANY",0
        even
        dc.w    LEGENDTOP+LEGENDSTEP*10+20
        dc.b    "TIMES IN LINES:",0
        even
        dc.w    LEGENDTOP+LEGENDSTEP*10+30
        dc.b    "454 CYCLES OF A",0
        even
        dc.w    LEGENDTOP+LEGENDSTEP*10+40
        dc.b    "7.09 MHZ 68000",0
        even
        dc.w    LEGENDTOP+LEGENDSTEP*10+56
        dc.b    "CLEARS 1536 BYTES",0
        even
        dc.w    LEGENDTOP+LEGENDSTEP*10+66
        dc.b    "MULTIPLIES 256",0
        even
        dc.w    -1

; An 8 by 8 font for the characters from the space to the underline (ASCII 32 to 95), made for How To Code
; v8: one byte a row, the top row first.
Font:
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; space
        dc.b    $10,$10,$10,$10,$10,$00,$10,$00   ; !
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; "
        dc.b    $28,$28,$7c,$28,$7c,$28,$28,$00   ; #
        dc.b    $10,$3c,$50,$38,$14,$78,$10,$00   ; $
        dc.b    $60,$64,$08,$10,$20,$4c,$0c,$00   ; %
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; &
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; '
        dc.b    $08,$10,$20,$20,$20,$10,$08,$00   ; (
        dc.b    $20,$10,$08,$08,$08,$10,$20,$00   ; )
        dc.b    $00,$10,$54,$38,$54,$10,$00,$00   ; *
        dc.b    $00,$10,$10,$7c,$10,$10,$00,$00   ; +
        dc.b    $00,$00,$00,$00,$18,$08,$10,$00   ; ,
        dc.b    $00,$00,$00,$7c,$00,$00,$00,$00   ; -
        dc.b    $00,$00,$00,$00,$00,$30,$30,$00   ; .
        dc.b    $00,$04,$08,$10,$20,$40,$00,$00   ; /
        dc.b    $38,$44,$4c,$54,$64,$44,$38,$00   ; 0
        dc.b    $10,$30,$10,$10,$10,$10,$38,$00   ; 1
        dc.b    $38,$44,$04,$08,$10,$20,$7c,$00   ; 2
        dc.b    $7c,$08,$10,$08,$04,$44,$38,$00   ; 3
        dc.b    $08,$18,$28,$48,$7c,$08,$08,$00   ; 4
        dc.b    $7c,$40,$78,$04,$04,$44,$38,$00   ; 5
        dc.b    $18,$20,$40,$78,$44,$44,$38,$00   ; 6
        dc.b    $7c,$04,$08,$10,$20,$20,$20,$00   ; 7
        dc.b    $38,$44,$44,$38,$44,$44,$38,$00   ; 8
        dc.b    $38,$44,$44,$3c,$04,$08,$30,$00   ; 9
        dc.b    $00,$30,$30,$00,$30,$30,$00,$00   ; :
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ;
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; <
        dc.b    $00,$00,$7c,$00,$7c,$00,$00,$00   ; =
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; >
        dc.b    $38,$44,$04,$08,$10,$00,$10,$00   ; ?
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; @
        dc.b    $38,$44,$44,$7c,$44,$44,$44,$00   ; A
        dc.b    $78,$44,$44,$78,$44,$44,$78,$00   ; B
        dc.b    $38,$44,$40,$40,$40,$44,$38,$00   ; C
        dc.b    $70,$48,$44,$44,$44,$48,$70,$00   ; D
        dc.b    $7c,$40,$40,$78,$40,$40,$7c,$00   ; E
        dc.b    $7c,$40,$40,$78,$40,$40,$40,$00   ; F
        dc.b    $38,$44,$40,$5c,$44,$44,$3c,$00   ; G
        dc.b    $44,$44,$44,$7c,$44,$44,$44,$00   ; H
        dc.b    $38,$10,$10,$10,$10,$10,$38,$00   ; I
        dc.b    $1c,$08,$08,$08,$08,$48,$30,$00   ; J
        dc.b    $44,$48,$50,$60,$50,$48,$44,$00   ; K
        dc.b    $40,$40,$40,$40,$40,$40,$7c,$00   ; L
        dc.b    $44,$6c,$54,$54,$44,$44,$44,$00   ; M
        dc.b    $44,$44,$64,$54,$4c,$44,$44,$00   ; N
        dc.b    $38,$44,$44,$44,$44,$44,$38,$00   ; O
        dc.b    $78,$44,$44,$78,$40,$40,$40,$00   ; P
        dc.b    $38,$44,$44,$44,$54,$48,$34,$00   ; Q
        dc.b    $78,$44,$44,$78,$50,$48,$44,$00   ; R
        dc.b    $3c,$40,$40,$38,$04,$04,$78,$00   ; S
        dc.b    $7c,$10,$10,$10,$10,$10,$10,$00   ; T
        dc.b    $44,$44,$44,$44,$44,$44,$38,$00   ; U
        dc.b    $44,$44,$44,$44,$44,$28,$10,$00   ; V
        dc.b    $44,$44,$44,$54,$54,$54,$28,$00   ; W
        dc.b    $44,$44,$28,$10,$28,$44,$44,$00   ; X
        dc.b    $44,$44,$28,$10,$10,$10,$10,$00   ; Y
        dc.b    $7c,$04,$08,$10,$20,$40,$7c,$00   ; Z
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; [
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; \
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ]
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ^
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; _

        section bss,bss

TimeText:       ds.b    6
HexText:        ds.b    10
        even
Numbers:        ds.w    COUNT
Times40:        ds.w    COUNT
Products:       ds.w    COUNT
ClearAny:       ds.b    CLEARBYTES      ; in any memory: fast RAM if there is some
GeneratedCode:  ds.b    CLEARBYTES/MOVEMBYTES*4+2

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$3200           ; three bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0fff           ; the ruler
        dc.w    COLOR02,$0223           ; the panel
        dc.w    COLOR03,$0fff           ; text on the panel
        dc.w    COLOR04,$0000
        dc.w    COLOR05,$0000
        dc.w    COLOR06,$0000           ; the swatches: set for each row below
        dc.w    COLOR07,$0fff
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
CopperSwatches:
        ; Two lines above each legend row, colour 6 becomes that test's colour (filled in by Init).
SWATCHWAIT      set     FIRSTLINE+LEGENDTOP-2
        rept    10
        dc.w    (SWATCHWAIT<<8)!$07,$fffe,COLOR06,0
SWATCHWAIT      set     SWATCHWAIT+LEGENDSTEP
        endr
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screen,bss_c

Plane1:         ds.b    PLANEBYTES      ; text and the ruler
Plane2:         ds.b    PLANEBYTES      ; the panel
Plane3:         ds.b    PLANEBYTES      ; the swatches
ClearChip:      ds.b    CLEARBYTES      ; what the clears clear: chip RAM, so the blitter can reach it
