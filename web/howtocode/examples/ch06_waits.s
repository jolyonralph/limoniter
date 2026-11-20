; ch06_waits.s - where a copper WAIT really happens: three bands of colour changes to look at, on a screen of
; one-pixel lines that mark every 16th pixel of the usual 320-pixel display window. Left mouse button to leave.
;
;   Top band: each line's colour is set after WAIT $xx01 (the Hardware Reference Manual's "start of the line").
;             The new colour shows a few pixels before the right-hand edge of the line before: look for the
;             step at the far right.
;   Middle:   the same with WAIT $xx07 (How To Code v7's tip). The colour changes in the horizontal blank,
;             where it can't be seen: no step.
;   Bottom:   a ruler. On each of its lines the colour changes at horizontal positions $10, $20, ... $E0, so you
;             can see where each position falls on the screen.
;
; How To Code v8, chapter 6. CC0: use it for anything.
;
; $VER: ch06_waits 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the bitplane's address into the copper list, and the lines drawn.

Init:
        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
.pal:
        ; Every row: a pixel at the left of each 16-pixel word, and one at the very right (pixel 319).
        lea     Screen,a0
        move.w  #HEIGHT-1,d7
.row:
        moveq   #ROWBYTES/2-1,d6
.word:
        move.w  #$8000,(a0)+
        dbf     d6,.word
        or.w    #$0001,-2(a0)
        dbf     d7,.row
        rts

; Nothing moves.
Frame:
        rts

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

RED             equ     $0d22
BLUE            equ     $023d

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
        dc.w    COLOR01,$0fff
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0

        ; The top band, lines $40 to $5F: WAIT $xx01, then a colour, red and blue in turn.
LINE    set     $40
        rept    32
        dc.w    (LINE<<8)!$01,$fffe
        dc.w    COLOR00,RED+(LINE&1)*(BLUE-RED)
LINE    set     LINE+1
        endr
        dc.w    $6007,$fffe,COLOR00,$0000

        ; The middle band, lines $70 to $8F: the same, with WAIT $xx07.
LINE    set     $70
        rept    32
        dc.w    (LINE<<8)!$07,$fffe
        dc.w    COLOR00,RED+(LINE&1)*(BLUE-RED)
LINE    set     LINE+1
        endr
        dc.w    $9007,$fffe,COLOR00,$0000

        ; The ruler, lines $A0 to $AF: yellow from positions $10, $30, $50...; green from $20, $40, $60...
LINE    set     $a0
        rept    16
H       set     $10
        rept    14
        dc.w    (LINE<<8)!H!$01,$fffe
        dc.w    COLOR00,$00c0+((H>>4)&1)*($0cc0-$00c0)
H       set     H+$10
        endr
LINE    set     LINE+1
        endr
        dc.w    $b007,$fffe,COLOR00,$0000

        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
