; ch07_clear_timing.s - how long it takes to clear 10K of chip RAM: by the blitter, by the processor, and by both
; at once, each half. Each is timed with the background colour, as coders always have: the colour changes as it
; starts and changes back as it ends, so the taller the band, the longer it took. The marks on the left are every
; 10 lines (64 microseconds each); the long ones every 50. Left mouse button to leave.
;
;   red     the blitter alone (D only, minterm 0), the processor waiting for it
;   green   the processor alone, with MOVEM.L
;   blue    both at once: the blitter clears the first half while the processor clears the second
;
; How To Code v8, chapter 7. CC0: use it for anything.
;
; $VER: ch07_clear_timing 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8
CLEARROWS       equ     256             ; what's cleared: a bitplane of 320 by 256, 10240 bytes
REDLINE         equ     60              ; the beam's line where each band starts
GREENLINE       equ     130
BLUELINE        equ     200

        section code,code

; ---------------------------------------------------------------------------------------------------------------

Init:
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
        move.l  #Ruler,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; The ruler, by the processor: a mark every 10 lines from the top of the display, longer every 50.
        ; The screen starts at line $2c (44), so its line n is the beam's line n + 44.
        lea     Ruler,a0
        move.w  #HEIGHT-1,d7
        moveq   #$2c,d6                 ; the beam's line
.line:
        moveq   #0,d0                   ; no mark
        moveq   #0,d1
        move.w  d6,d1
        divu.w  #10,d1                  ; the remainder is in the top word
        swap    d1
        tst.w   d1
        bne.s   .nomark
        move.w  #$ff00,d0               ; a short mark
        moveq   #0,d1
        move.w  d6,d1
        divu.w  #50,d1
        swap    d1
        tst.w   d1
        bne.s   .nomark
        move.w  #$ffff,d0               ; a long one
.nomark:
        move.w  d0,(a0)
        lea     ROWBYTES(a0),a0
        addq.w  #1,d6
        dbf     d7,.line
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the three clears, each at its line.

Frame:
        BLIT_BEGIN

        ; Red: the blitter alone.
        move.w  #REDLINE,d0
        bsr     WaitLine
        move.w  #$0f00,COLOR00(a5)
        lea     Buffer,a0
        move.w  #CLEARROWS,d0
        bsr     BlitterClear
        jsr     WaitBlitter
        move.w  #$0000,COLOR00(a5)

        ; Green: the processor alone.
        move.w  #GREENLINE,d0
        bsr     WaitLine
        move.w  #$00f0,COLOR00(a5)
        lea     Buffer+CLEARROWS*ROWBYTES,a0 ; the end: it clears backwards
        move.w  #CLEARROWS,d0
        bsr     ProcessorClear
        move.w  #$0000,COLOR00(a5)

        ; Blue: both. Start the blitter on the first half, clear the second half with the processor meanwhile,
        ; then wait for the blitter (it may finish first, or not).
        move.w  #BLUELINE,d0
        bsr     WaitLine
        move.w  #$000f,COLOR00(a5)
        lea     Buffer,a0
        move.w  #CLEARROWS/2,d0
        bsr     BlitterClear
        lea     Buffer+CLEARROWS*ROWBYTES,a0
        move.w  #CLEARROWS/2,d0
        bsr     ProcessorClear
        jsr     WaitBlitter
        move.w  #$0000,COLOR00(a5)

        BLIT_END
        rts

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits until the beam reaches line d0 (0 to 255), with a5 = CUSTOM.

WaitLine:
.wait:
        move.l  VPOSR(a5),d1
        lsr.l   #8,d1
        and.w   #$1ff,d1
        cmp.w   d0,d1
        blo.s   .wait
        rts

; ---------------------------------------------------------------------------------------------------------------
; Starts the blitter clearing d0 rows of 40 bytes from a0: D only, minterm 0 (D is always 0). Doesn't wait.

BlitterClear:
        jsr     WaitBlitter
        move.l  #$01000000,BLTCON0(a5)  ; BLTCON0 $0100: D only, LF 0; BLTCON1 0
        move.w  #0,BLTDMOD(a5)
        move.l  a0,BLTDPTH(a5)
        lsl.w   #6,d0
        or.w    #ROWBYTES/2,d0
        move.w  d0,BLTSIZE(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Clears d0 rows of 40 bytes backwards from a0 (the end), with the processor: ten registers of zeros, so each
; MOVEM.L clears a row. d0 must be a multiple of 4. Keeps a5.

ProcessorClear:
        movem.l d2-d7/a2/a6,-(sp)
        move.l  a0,a6
        move.w  d0,d7
        lsr.w   #2,d7                   ; four rows each time round
        subq.w  #1,d7
        moveq   #0,d0
        moveq   #0,d1
        moveq   #0,d2
        moveq   #0,d3
        moveq   #0,d4
        moveq   #0,d5
        moveq   #0,d6
        move.l  d0,a0
        move.l  d0,a1
        move.l  d0,a2
.rows:
        movem.l d0-d6/a0-a2,-(a6)       ; 40 bytes
        movem.l d0-d6/a0-a2,-(a6)
        movem.l d0-d6/a0-a2,-(a6)
        movem.l d0-d6/a0-a2,-(a6)
        dbf     d7,.rows
        movem.l (sp)+,d2-d7/a2/a6
        rts

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane: the ruler
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0aaa
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screens,bss_c

Ruler:          ds.b    ROWBYTES*HEIGHT
Buffer:         ds.b    ROWBYTES*CLEARROWS ; cleared and cleared again, never shown
