; ch08_interlace.s - a high resolution interlaced screen, 640 by 512: fine rings that only come out round and
; smooth if each field shows the right lines. The long frames show the picture's even rows and the short frames
; its odd rows, from two copper lists; each frame the interrupt reads which kind of frame this is, and points the
; copper at the other list for the next. Left mouse button to leave.
;
; How To Code v8, chapter 8. CC0: use it for anything.
;
; $VER: ch08_interlace 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     640
HEIGHT          equ     512             ; PAL; NTSC shows 400
ROWBYTES        equ     WIDTH/8

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines a field
        move.w  #$f4c1,CopperShortWindowEnd+2
        move.l  #(512-400)/2*ROWBYTES,d0        ; and the middle 400 rows of the picture
        bra.s   .pointers
.pal:
        moveq   #0,d0
.pointers:
        ; The long frame's list starts at row 0, the short frame's at row 1. The modulo of one row (80 bytes)
        ; skips the other field's rows.
        add.l   #Picture,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2
        swap    d0
        add.l   #ROWBYTES,d0
        move.w  d0,CopperShortBitplane+6
        swap    d0
        move.w  d0,CopperShortBitplane+2

        bsr     DrawRings
        rts

; Rings round the middle: a pixel is set where its distance squared from the middle, in half pixels, divided by
; 16384, is odd. The top half is worked out (the square kept up to date as x moves on, as in ch06_split.s), and
; the bottom half is the top half upside down.
DrawRings:
        lea     Picture,a0
        moveq   #0,d7                   ; the row
.row:
        move.w  d7,d2
        add.w   d2,d2
        sub.w   #HEIGHT-1,d2            ; dy, in half pixels
        muls    d2,d2
        move.w  #-(WIDTH-1),d1          ; dx, in half pixels
        move.l  #(WIDTH-1)*(WIDTH-1),d0 ; dx squared
        moveq   #ROWBYTES/2-1,d6
.word:
        moveq   #16-1,d5
.pixel:
        move.l  d0,d4
        add.l   d2,d4
        add.w   d3,d3
        btst    #14,d4
        beq.s   .clear
        addq.w  #1,d3
.clear:
        move.w  d1,d4                   ; (dx+2)^2 = dx^2 + 4dx + 4
        ext.l   d4
        add.l   d4,d4
        add.l   d4,d4
        add.l   d4,d0
        addq.l  #4,d0
        addq.w  #2,d1
        dbf     d5,.pixel
        move.w  d3,(a0)+
        dbf     d6,.word
        addq.w  #1,d7
        cmp.w   #HEIGHT/2,d7
        blo.s   .row

        ; The bottom half: row r is row HEIGHT-1-r.
        lea     Picture+(HEIGHT/2-1)*ROWBYTES,a1        ; the top half's last row
        move.w  #HEIGHT/2-1,d7
.mirror:
        moveq   #ROWBYTES/4-1,d6
.long:
        move.l  (a1)+,(a0)+
        dbf     d6,.long
        sub.w   #ROWBYTES*2,a1          ; back to the start of this row, and up one
        dbf     d7,.mirror
        rts

Frame:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of each frame. The copper has already started this frame from COP1LC, so this chooses the list for
; the next. VPOSR's top bit (LOF) is 1 in a long frame; with interlace on, long and short frames take turns, so if
; this one is long the next is short.

VBlank:
        btst    #7,VPOSR(a5)            ; bit 15 of the word: bit 7 of its first byte
        beq.s   .short
        move.l  #CopperShort,COP1LC(a5)
        rts
.short:
        move.l  #Copper,COP1LC(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; The two copper lists: the same but for where the bitplane starts.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; the window is given in non-interlaced lines, as always
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$003c           ; high resolution's fetch
        dc.w    DDFSTOP,$00d4
        dc.w    BPLCON0,$9204           ; high resolution, one bitplane, colour, interlace (bit 2)
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWBYTES        ; skip a row: it's the other field's
        dc.w    BPL2MOD,ROWBYTES
        dc.w    COLOR00,$0013
        dc.w    COLOR01,$0bdf
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0     ; row 0
        dc.w    $ffff,$fffe

CopperShort:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperShortWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$003c
        dc.w    DDFSTOP,$00d4
        dc.w    BPLCON0,$9204
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWBYTES
        dc.w    BPL2MOD,ROWBYTES
        dc.w    COLOR00,$0013
        dc.w    COLOR01,$0bdf
CopperShortBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0     ; row 1
        dc.w    $ffff,$fffe

        section screen,bss_c

Picture:        ds.b    ROWBYTES*HEIGHT
