; ch09_256colours.s - AGA only: 8 bitplanes, 256 colours on screen at once, every one of them a 24-bit colour,
; fetched 64 bits at a time (FMODE 4x). The picture is drawn in chunky form (one byte a pixel) and turned into
; bitplanes once, at the start; the copper sets all 256 colours below the display each frame, and the palette
; cycles. On a machine without AGA it shows nothing and ends at once. Left mouse button to leave.
;
; How To Code v8, chapter 9. CC0: use it for anything.
;
; $VER: ch09_256colours 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8         ; 40: a multiple of 8, as 4x fetch needs
PLANES          equ     8
ROWSTRIDE       equ     ROWBYTES*PLANES ; interleaved: each row of the screen has all eight planes' rows in turn
COLOURS         equ     256
BANKWORDS       equ     2+32*2+2+32*2   ; the copper words for one bank of 32 colours (see MakeCopperPalette)


        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; AGA or not? Without it, none of this can be shown: end the copper list after its first move (no
        ; bitplanes) and leave at once.
        bsr     IsAGA
        bne.s   .aga
        move.l  #$fffffffe,CopperAGA
        st      ExitNow
        rts
.aga:
        ; The screen, on an 8-byte boundary: 4x fetch reads 64 bits at a time, and can't start part way into
        ; them. Interleaved, every plane's start (Screen + plane * 40) is on one too, and so is the modulo.
        move.l  #ScreenSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,ScreenStart
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        add.l   #ROWBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane

        ; NTSC: 200 lines, so the display ends at line $f4, and the palette goes in below that instead.
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
        move.l  #$f407fffe,CopperPaletteWait
        move.l  #$f407fffe,CopperPaletteWait+4 ; the same line again: passes straight on
.pal:

        bsr     MakePalette
        bsr     MakeCopperPalette
        bsr     DrawChunky
        bsr     ChunkyToPlanar
        rts

; ---------------------------------------------------------------------------------------------------------------
; Is this an AGA machine? Returns d0 = 1 (and the Z flag clear) if so, 0 if not.
;
; Lisa, AGA's video chip, answers a read of DENISEID ($DFF07C) with $F8 in the low byte. ECS's Denise answers $FC.
; The first Denise has no such register: a read gives whatever happens to be on the bus, which could be $F8 by
; chance. So read it many times: a chip that's really there gives the same answer every time.

IsAGA:
        move.w  DENISEID(a5),d0
        cmp.b   #$f8,d0
        bne.s   .no
        moveq   #32-1,d1
.again:
        move.w  DENISEID(a5),d2
        cmp.b   d0,d2
        bne.s   .no
        dbf     d1,.again
        moveq   #1,d0
        rts
.no:
        moveq   #0,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The palette: 256 colours of 24 bits, smooth steps between eight key colours, 32 steps from each to the next.
; The last key leads back to the first, so the palette wraps round without a jump as it cycles. Each colour is
; kept as the two 12-bit halves the colour registers take: the high nibble of red, green and blue, then the low.

MakePalette:
        lea     KeyColours,a0
        lea     Palette,a1
        moveq   #8-1,d7                 ; eight keys
.key:
        move.l  (a0)+,d0                ; from this colour ($00RRGGBB)
        move.l  (a0),d1                 ; to the next
        tst.w   d7
        bne.s   .notlast
        move.l  KeyColours,d1           ; the last key leads back to the first
.notlast:
        moveq   #0,d2                   ; the step, 0 to 31
.step:
        moveq   #0,d6                   ; the 24-bit colour being made
        moveq   #16,d5                  ; red (bits 16-23), then green (8-15), then blue (0-7)
.component:
        move.l  d0,d3
        lsr.l   d5,d3
        and.w   #$ff,d3                 ; from
        move.l  d1,d4
        lsr.l   d5,d4
        and.w   #$ff,d4                 ; to
        sub.w   d3,d4
        muls    d2,d4
        asr.l   #5,d4                   ; (to - from) * step / 32
        add.w   d3,d4
        and.l   #$ff,d4
        lsl.l   d5,d4
        or.l    d4,d6
        subq.w  #8,d5
        bpl.s   .component
        ; Split it: $00RrGgBb becomes $0RGB (the high nibbles) and $0rgb (the low).
        move.l  d6,d3
        lsr.l   #4,d3
        move.w  d3,d4
        and.w   #$000f,d4               ; blue's high nibble
        lsr.l   #4,d3
        move.w  d3,d5
        and.w   #$00f0,d5               ; green's
        or.w    d5,d4
        lsr.l   #4,d3
        and.w   #$0f00,d3               ; red's
        or.w    d3,d4
        move.w  d4,(a1)+
        move.w  d6,d4
        and.w   #$000f,d4               ; blue's low nibble
        move.l  d6,d3
        lsr.l   #4,d3
        move.w  d3,d5
        and.w   #$00f0,d5               ; green's
        or.w    d5,d4
        lsr.l   #4,d3
        and.w   #$0f00,d3               ; red's
        or.w    d3,d4
        move.w  d4,(a1)+
        addq.w  #1,d2
        cmp.w   #32,d2
        blo.s   .step
        dbf     d7,.key
        rts

; ---------------------------------------------------------------------------------------------------------------
; The copper's part of the palette: for each of the eight banks of 32 colours, select the bank, write the 32
; high halves, set LOCT, write the 32 low halves. The high half has to go first: writing it (with LOCT clear)
; sets the low half too, to the same nibbles, so that a 12-bit colour written the old way looks the same on AGA.

MakeCopperPalette:
        lea     CopperPalette,a0
        moveq   #0,d0                   ; the bank, 0 to 7
.bank:
        move.w  d0,d1
        ror.w   #3,d1                   ; the bank in bits 13-15 of BPLCON3
        or.w    #$0c20,d1               ; with the rest as at the top of the list
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+                ; LOCT clear: the high halves
        bsr.s   .registers
        or.w    #$0200,d1               ; LOCT set: the low halves
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+
        bsr.s   .registers
        addq.w  #1,d0
        cmp.w   #8,d0
        blo.s   .bank
        rts
.registers:
        move.w  #COLOR00,d2
        moveq   #32-1,d3
.register:
        move.w  d2,(a0)+
        clr.w   (a0)+                   ; the value: filled in each frame
        addq.w  #2,d2
        dbf     d3,.register
        rts

; ---------------------------------------------------------------------------------------------------------------
; The picture, in chunky form: one byte for each pixel, its colour number. A zone plate: the colour is the
; square of the distance from the middle, so the rings come closer together further out.

DrawChunky:
        lea     Chunky,a0
        move.w  #-HEIGHT/2,d7           ; y, from the middle
.row:
        move.w  d7,d6
        muls    d6,d6                   ; y * y
        move.w  #-WIDTH/2,d5            ; x, from the middle
.pixel:
        move.w  d5,d4
        muls    d4,d4                   ; x * x
        add.l   d6,d4
        lsr.l   #6,d4                   ; (x * x + y * y) / 64: about 2.5 times round the palette to the corners
        move.b  d4,(a0)+
        addq.w  #1,d5
        cmp.w   #WIDTH/2,d5
        blt.s   .pixel
        addq.w  #1,d7
        cmp.w   #HEIGHT/2,d7
        blt.s   .row
        rts

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar: the 8 bits of each pixel's colour go to 8 bitplanes, bit 0 to plane 1 and so on. This is the
; plainest way, done once (it takes a fraction of a second on an A1200); the chapter says how it's done fast
; enough to do every frame.
;
; A table gives, for each colour, its 8 bits spread out one to a byte: plane 1's bit in the top byte of the
; first long, plane 4's in the bottom byte, then planes 5 to 8 in the second long. Eight pixels make one byte
; of each plane: shift the two longs left a bit, OR in the next pixel's longs, eight times.

ChunkyToPlanar:
        lea     BitsTable,a0
        moveq   #0,d0                   ; the colour
.entry:
        moveq   #0,d1                   ; planes 1-4
        moveq   #0,d2                   ; planes 5-8
        moveq   #0,d3                   ; the bit, 0 to 3 (and 4 to 7)
.bit:
        lsl.l   #8,d1
        lsl.l   #8,d2
        btst    d3,d0
        beq.s   .low0
        addq.b  #1,d1
.low0:
        addq.w  #4,d3
        btst    d3,d0
        beq.s   .high0
        addq.b  #1,d2
.high0:
        subq.w  #3,d3                   ; back 4, and on to the next bit
        cmp.w   #4,d3
        blo.s   .bit
        move.l  d1,(a0)+
        move.l  d2,(a0)+
        addq.w  #1,d0
        cmp.w   #COLOURS,d0
        blo.s   .entry

        lea     Chunky,a0
        move.l  ScreenStart,a1
        lea     BitsTable,a2
        move.w  #HEIGHT-1,d7
.row:
        moveq   #ROWBYTES-1,d6
.byte:
        moveq   #0,d0
        moveq   #0,d1
        moveq   #8-1,d5
.pixel:
        moveq   #0,d2
        move.b  (a0)+,d2
        lsl.w   #3,d2                   ; 8 bytes in the table for each colour
        add.l   d0,d0
        add.l   d1,d1
        or.l    0(a2,d2.w),d0
        or.l    4(a2,d2.w),d1
        dbf     d5,.pixel
        rol.l   #8,d0
        move.b  d0,(a1)                 ; plane 1
        rol.l   #8,d0
        move.b  d0,ROWBYTES*1(a1)
        rol.l   #8,d0
        move.b  d0,ROWBYTES*2(a1)
        rol.l   #8,d0
        move.b  d0,ROWBYTES*3(a1)
        rol.l   #8,d1
        move.b  d1,ROWBYTES*4(a1)
        rol.l   #8,d1
        move.b  d1,ROWBYTES*5(a1)
        rol.l   #8,d1
        move.b  d1,ROWBYTES*6(a1)
        rol.l   #8,d1
        move.b  d1,ROWBYTES*7(a1)       ; plane 8
        addq.l  #1,a1
        dbf     d6,.byte
        add.w   #ROWSTRIDE-ROWBYTES,a1  ; on to the next row of plane 1
        dbf     d7,.row
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the palette moves a step, by writing every colour into the copper list again. The copper reads
; these below the display, so writing them now, near the top of the frame, never changes them half way through.

Frame:
        tst.b   ExitNow                 ; no AGA: nothing to do
        bne.s   .done
        move.l  Frames,d0
        moveq   #0,d1                   ; the colour number
        lea     CopperPalette,a0
        lea     Palette,a1
        moveq   #8-1,d7                 ; eight banks
.bank:
        moveq   #32-1,d6
        lea     6(a0),a2                ; the first high half's value: past BPLCON3 and the first register number
        lea     4+32*4+6(a0),a3         ; the first low half's
.colour:
        move.w  d1,d2
        add.w   d0,d2
        and.w   #COLOURS-1,d2
        lsl.w   #2,d2
        move.w  0(a1,d2.w),(a2)         ; high half
        move.w  2(a1,d2.w),(a3)         ; low half
        addq.l  #4,a2
        addq.l  #4,a3
        addq.w  #1,d1
        dbf     d6,.colour
        lea     BANKWORDS*2(a0),a0
        dbf     d7,.bank
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do here.

VBlank:
        rts

        section data,data

; The key colours, 24 bits each: night blue, sea, ice, white, gold, flame, rose, violet.
KeyColours:
        dc.l    $000c1a40,$00127a9a,$0070d0e0,$00f4f4f0
        dc.l    $00f0c040,$00e05010,$00a01050,$00381060

        section bss,bss

ScreenStart:    ds.l    1
Palette:        ds.w    COLOURS*2       ; each colour's high half, then its low half
BitsTable:      ds.l    COLOURS*2
Chunky:         ds.b    WIDTH*HEIGHT

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM.

        section copper,data_c

Copper:
        dc.w    BPLCON0,$0200           ; no bitplanes until the rest is ready (on a machine without AGA, this
                                        ; is all there is)
CopperAGA:
        dc.w    FMODE,$0003             ; bitplanes fetched 64 bits at a time (4x); sprites 16 pixels wide
        dc.w    BPLCON3,$0c20           ; colour bank 0, LOCT clear, sprites at their usual resolution, and
                                        ; BRDRBLNK: the border black, whatever colour 0 is
        dc.w    BPLCON4,$0011           ; no XOR on the colours; sprites take colours 16-31, as they always have
        dc.w    DIWSTRT,$2c81           ; a 320 by 256 window, the usual PAL screen
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038           ; the first fetch at $38, as for 1x; with 4x, 5 fetches of 64 pixels,
        dc.w    DDFSTOP,$00b8           ; $20 apart, so the last at $b8 (not $d0: that's 1x's last word)
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWSTRIDE-ROWBYTES ; skip the other seven planes' rows: 280, a multiple of 8
        dc.w    BPL2MOD,ROWSTRIDE-ROWBYTES
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
        dc.w    BPL5PTH,0,BPL5PTL,0
        dc.w    BPL6PTH,0,BPL6PTL,0
        dc.w    BPL7PTH,0,BPL7PTH+2,0
        dc.w    BPL8PTH,0,BPL8PTH+2,0
        dc.w    BPLCON0,$0211           ; 8 bitplanes: BPU3 (bit 4) set, BPU0-2 (bits 12-14) clear; colour on;
                                        ; ECSENA (bit 0), without which BPLCON3's BRDRBLNK does nothing
CopperPaletteWait:
        dc.w    $ffdf,$fffe             ; past line 255
        dc.w    $2c07,$fffe             ; line 300 ($12c): the display has ended
CopperPalette:
        ds.w    BANKWORDS*8             ; filled in by MakeCopperPalette, and the colours by Frame
        dc.w    BPLCON3,$0c20           ; bank 0 and LOCT clear again, as everything else expects
        dc.w    $ffff,$fffe

        section screen,bss_c

ScreenSpace:
        ds.b    ROWSTRIDE*HEIGHT+8      ; 8 bytes more, so it can start on an 8-byte boundary
