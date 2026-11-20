; startup_demo.s - the smallest whole demo on How To Code v8's startup: one bitplane with a chequerboard, and a
; raster bar bouncing over it. Left mouse button to leave.
;
; How To Code v8, chapter 4. CC0: use it for anything.
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath.
;
; $VER: startup_demo 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8
BARLINES        equ     8               ; how tall the raster bar is
BARTOP          equ     $40             ; how high and low it bounces, in display lines ($2c is the top line)
BARBOTTOM       equ     $f0

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; The bitplane's address into the copper list, high word then low word.
        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; All eight sprites at an empty sprite, so none shows even with sprite DMA on.
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

        ; NTSC has 200 lines: the display window ends at line $f4 instead of $2c (that is, $12c).
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
.pal:

        ; Clear the screen with the blitter: D only, all zeros (minterm 0), a whole bitplane in one blit.
        BLIT_BEGIN
        move.l  #Screen,BLTDPTH(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  #$01000000,BLTCON0(a5)  ; BLTCON0 = $0100 (D on, minterm 0); BLTCON1 = 0
        move.w  #(HEIGHT<<6)!(ROWBYTES/2),BLTSIZE(a5)
        BLIT_END

        ; Then the chequerboard, 16 by 16 squares, by the processor: rows of $FFFF0000, swapped every 16 rows.
        lea     Screen,a0
        move.w  #HEIGHT-1,d7
.row:
        move.l  #$ffff0000,d1
        btst    #4,d7
        beq.s   .fill
        not.l   d1
.fill:
        moveq   #ROWBYTES/4-1,d6
.long:
        move.l  d1,(a0)+
        dbf     d6,.long
        dbf     d7,.row
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the raster bar moves a line, by rewriting the line numbers in its copper waits.

Frame:
        move.w  BarLine,d0
        add.w   BarStep,d0
        cmp.w   #BARTOP,d0
        ble.s   .turn
        cmp.w   #BARBOTTOM-BARLINES,d0
        blt.s   .move
.turn:
        neg.w   BarStep
.move:
        move.w  d0,BarLine
        lea     CopperBar,a0
        moveq   #BARLINES,d1            ; BARLINES+1 waits: dbf counts down to -1
.line:
        move.b  d0,(a0)
        addq.w  #1,d0
        addq.l  #8,a0
        dbf     d1,.line
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do here yet.

VBlank:
        rts

        section data,data

BarLine:        dc.w    BARTOP+1
BarStep:        dc.w    1

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM, where the chips can read them.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; a 320 by 256 window, the usual PAL screen
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0368
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
CopperBar:
        ; Wait for the line, from its left edge ($07, so the colour changes as the line starts, not at the end of
        ; the line before), then set the background colour.
        dc.w    $4107,$fffe,COLOR00,$0102
        dc.w    $4207,$fffe,COLOR00,$0315
        dc.w    $4307,$fffe,COLOR00,$062a
        dc.w    $4407,$fffe,COLOR00,$0c4f
        dc.w    $4507,$fffe,COLOR00,$0c4f
        dc.w    $4607,$fffe,COLOR00,$062a
        dc.w    $4707,$fffe,COLOR00,$0315
        dc.w    $4807,$fffe,COLOR00,$0102
        dc.w    $4907,$fffe,COLOR00,$0000
        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

NoSprite:
        dc.l    0,0

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
