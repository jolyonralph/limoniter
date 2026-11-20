; ch06_gradient.s - a copper gradient behind a city skyline: the background colour is set again on every one of
; the 256 lines, from a palette of 128 colours that cycles a step each frame. The list crosses line 255 the right
; way. Left mouse button to leave.
;
; How To Code v8, chapter 6. CC0: use it for anything.
;
; $VER: ch06_gradient 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows 200 lines, so the picture starts lower down
ROWBYTES        equ     WIDTH/8
TOPLINE         equ     $2c             ; the display's first line
LINES           equ     256             ; how many lines the gradient covers
COLOURS         equ     128             ; how many colours in the palette (a power of two, to wrap with AND)

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; NTSC: 200 lines, so show the bottom 200 rows of the picture, where the skyline is.
        move.l  #Screen,d0
        tst.b   PalDisplay
        bne.s   .pal
        add.l   #(HEIGHT-200)*ROWBYTES,d0
        move.w  #$f4c1,CopperWindowEnd+2
.pal:
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        bsr     MakePalette
        bsr     MakeCopperLines
        bsr     DrawSkyline
        rts

; ---------------------------------------------------------------------------------------------------------------
; The palette: smooth steps between a few key colours, 16 steps from each to the next. The last key is the first
; again, so the palette wraps round without a jump as it cycles.

MakePalette:
        lea     KeyColours,a0
        lea     Palette,a1
.key:
        move.w  (a0)+,d0                ; from this colour
        move.w  (a0),d1                 ; to the next
        bmi.s   .done                   ; -1 ends the keys
        moveq   #0,d2                   ; the step, 0 to 15
.step:
        moveq   #0,d6                   ; the colour being made
        moveq   #8,d5                   ; red (bits 8-11), then green (4-7), then blue (0-3)
.component:
        move.w  d0,d3
        lsr.w   d5,d3
        and.w   #$f,d3                  ; from
        move.w  d1,d4
        lsr.w   d5,d4
        and.w   #$f,d4                  ; to
        sub.w   d3,d4
        muls    d2,d4
        asr.w   #4,d4                   ; (to - from) * step / 16
        add.w   d3,d4
        lsl.w   d5,d4
        or.w    d4,d6
        subq.w  #4,d5
        bpl.s   .component
        move.w  d6,(a1)+
        addq.w  #1,d2
        cmp.w   #16,d2
        blo.s   .step
        bra.s   .key
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The copper list's gradient part: for each line, WAIT for it, then MOVE a colour to COLOR00. The colours are
; filled in by Frame.
;
; A WAIT holds only the low 8 bits of the line, so lines 256 and on are written as 0 and on. Before the first of
; them comes WAIT $FFDF: the end of line 255. Without it, "WAIT $0007" (line 256) would pass at once, on line
; 255 or even at the top, as $FF and everything before it are already "greater than or equal to" $00.

MakeCopperLines:
        lea     CopperLines,a0
        move.w  #TOPLINE,d0
.line:
        cmp.w   #$100,d0
        bne.s   .notwrap
        move.l  #$ffdffffe,(a0)+        ; wait for the end of line 255
.notwrap:
        move.w  d0,d1
        lsl.w   #8,d1                   ; the line's low 8 bits, in the WAIT's top byte
        or.w    #$07,d1                 ; $07: the colour changes in the horizontal blank
        move.w  d1,(a0)+
        move.w  #$fffe,(a0)+
        move.w  #COLOR00,(a0)+
        clr.w   (a0)+
        addq.w  #1,d0
        cmp.w   #TOPLINE+LINES,d0
        blo.s   .line
        ; Below the gradient, black again (on NTSC this line never comes, which does no harm).
        move.w  d0,d1
        lsl.w   #8,d1
        or.w    #$07,d1
        move.w  d1,(a0)+
        move.w  #$fffe,(a0)+
        move.l  #COLOR00<<16,(a0)+
        move.l  #$fffffffe,(a0)+        ; the end
        rts

; ---------------------------------------------------------------------------------------------------------------
; A skyline in the one bitplane: buildings of random widths and heights, with holes for windows, so the gradient
; shows through them.

DrawSkyline:
        moveq   #0,d4                   ; x: the building's left edge
.building:
        bsr     Random
        and.w   #15,d0
        add.w   #10,d0
        move.w  d0,d5                   ; its width: 10 to 25
        bsr     Random
        and.w   #127,d0
        add.w   #30,d0
        move.w  d0,d6                   ; its height: 30 to 157
        moveq   #0,d3                   ; the column within the building
.column:
        move.w  d4,d1
        add.w   d3,d1                   ; x on the screen
        cmp.w   #WIDTH,d1
        bhs.s   .done
        move.w  #HEIGHT-1,d2            ; y, from the bottom up
.pixel:
        ; A window: away from the building's edges, columns 2 and 3 of every 4, and rows 2 to 4 of every 7.
        cmp.w   #1,d3
        bls.s   .solid
        move.w  d5,d0
        subq.w  #2,d0
        cmp.w   d0,d3
        bhs.s   .solid
        move.w  d3,d0
        and.w   #3,d0
        cmp.w   #2,d0
        blo.s   .solid
        move.w  d2,d0
        ext.l   d0
        divu    #7,d0
        swap    d0                      ; y mod 7
        subq.w  #2,d0
        cmp.w   #2,d0
        bls.s   .next                   ; 2, 3 or 4: a window, left clear
.solid:
        move.w  d2,d0
        mulu    #ROWBYTES,d0
        move.w  d1,d7
        lsr.w   #3,d7
        add.w   d7,d0                   ; the byte
        lea     Screen,a0
        move.w  d1,d7
        not.w   d7                      ; bset counts from the right: 7 - (x and 7)
        bset    d7,(a0,d0.l)
.next:
        subq.w  #1,d2
        move.w  #HEIGHT,d0
        sub.w   d6,d0
        cmp.w   d0,d2
        bge.s   .pixel
        addq.w  #1,d3
        cmp.w   d5,d3
        blo.s   .column
        add.w   d5,d4
        addq.w  #2,d4                   ; a gap between buildings
        bra     .building
.done:
        rts

; A pseudo-random number in d0 (a linear congruential generator; the low bits are poor, so the high word is used).
Random:
        move.l  Seed,d0
        mulu    #25173,d0
        add.l   #13849,d0
        move.l  d0,Seed
        swap    d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: every line's colour moves one step along the palette (each palette colour covers two lines, so
; the 128 colours make one band down the whole screen). 256 words are written, top first, at the start of the
; frame: the 68000 is past the top line's colour well before the beam reaches line $2C.

Frame:
        addq.w  #1,Cycle
        move.w  Cycle,d1
        lea     Palette,a1
        lea     CopperLines+6,a0        ; the first line's colour: after its WAIT and the MOVE's register
        moveq   #0,d2                   ; the line, from the top of the gradient
.line:
        cmp.w   #$100-TOPLINE,d2
        bne.s   .notwrap
        addq.l  #4,a0                   ; past the WAIT $FFDF
.notwrap:
        move.w  d2,d0
        lsr.w   #1,d0
        sub.w   d1,d0                   ; minus the cycle: the colours move down the screen
        and.w   #COLOURS-1,d0
        add.w   d0,d0
        move.w  (a1,d0.w),(a0)
        addq.l  #8,a0
        addq.w  #1,d2
        cmp.w   #LINES,d2
        blo.s   .line
        rts

VBlank:
        rts

        section data,data

; From the top of the sky to the bottom, and round again: night, dusk, sunset, and back.
KeyColours:
        dc.w    $0002,$0118,$063a,$0c56,$0fb4,$0c56,$063a,$0118,$0002
        dc.w    -1                      ; 8 steps between 9 keys: 8 * 16 = COLOURS
Seed:           dc.l    1993
Cycle:          dc.w    0

        section palette,bss

Palette:        ds.w    COLOURS

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0000           ; the skyline: black against the sky
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
CopperLines:
        ds.l    LINES*2+1+4             ; a WAIT and a MOVE for each line, the WAIT $FFDF, and the end

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
