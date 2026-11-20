; ch15_memory_map.s - shows the memory this Amiga has, as exec.library knows it: a line for each region, with its
; kind (the MEMF_ bits), its first and last address, and how much of it is free; then how much chip and other
; memory is free in all. Left mouse button to leave.
;
; How To Code v8, chapter 15. CC0: use it for anything.
;
; Both ways. The numbers are hexadecimal; on each line: attributes, lower bound, upper bound (the first address
; past the region), bytes free. The last line: AvailMem(MEMF_CHIP), then AvailMem(MEMF_FAST).
;
; $VER: ch15_memory_map 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
MAXREGIONS      equ     12
TOP             equ     24              ; the first line of numbers
PITCH           equ     14              ; lines apart

_LVOAvailMem    equ     -216
MemList         equ     322             ; in ExecBase: the list of memory regions (private: read it, don't change it)
MH_ATTRIBUTES   equ     14              ; in each MemHeader
MH_LOWER        equ     20
MH_UPPER        equ     24
MH_FREE         equ     28

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: everything is found and drawn here; nothing changes after.

Init:
        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
.pal:
        move.l  ExecBase.w,a6
        ; The list is exec's own, and changes as memory is allocated: read it with multitasking off. (Taking the
        ; machine, it's off already; Forbid and Permit nest, so this is right either way.)
        jsr     _LVOForbid(a6)
        lea     Screen+TOP*ROWBYTES+1,a3
        moveq   #MAXREGIONS-1,d7
        move.l  MemList(a6),a2          ; the first region (lh_Head)
.region:
        tst.l   (a2)                    ; the list's end: a node whose next is 0
        beq.s   .done
        move.l  a3,a0
        move.w  MH_ATTRIBUTES(a2),d0
        moveq   #4,d1
        bsr     DrawHex
        addq.l  #1,a0
        move.l  MH_LOWER(a2),d0
        moveq   #8,d1
        bsr     DrawHex
        addq.l  #1,a0
        move.l  MH_UPPER(a2),d0
        moveq   #8,d1
        bsr     DrawHex
        addq.l  #1,a0
        move.l  MH_FREE(a2),d0
        moveq   #8,d1
        bsr     DrawHex
        lea     PITCH*ROWBYTES(a3),a3
        move.l  (a2),a2
        dbf     d7,.region
.done:
        jsr     _LVOPermit(a6)

        ; In all: AvailMem is the way to ask, with no private structures involved.
        lea     PITCH*ROWBYTES(a3),a3
        moveq   #MEMF_CHIP,d1
        jsr     _LVOAvailMem(a6)
        lea     5(a3),a0
        moveq   #8,d1
        bsr     DrawHex
        moveq   #MEMF_FAST,d1
        jsr     _LVOAvailMem(a6)
        lea     14(a3),a0
        moveq   #8,d1
        bsr     DrawHex
        rts

; d0 = a number, d1.w = how many hexadecimal digits of it (its lowest), a0 = where; a0 moves on past them.
DrawHex:
        moveq   #8,d2
        sub.w   d1,d2
        lsl.w   #2,d2
        lsl.l   d2,d0
        subq.w  #1,d1
.digit:
        rol.l   #4,d0
        moveq   #15,d2
        and.w   d0,d2
        lsl.w   #3,d2
        lea     Font,a1
        add.w   d2,a1
        move.b  (a1)+,(a0)
        move.b  (a1)+,ROWBYTES(a0)
        move.b  (a1)+,ROWBYTES*2(a0)
        move.b  (a1)+,ROWBYTES*3(a0)
        move.b  (a1)+,ROWBYTES*4(a0)
        move.b  (a1)+,ROWBYTES*5(a0)
        move.b  (a1)+,ROWBYTES*6(a0)
        move.b  (a1)+,ROWBYTES*7(a0)
        addq.l  #1,a0
        dbf     d1,.digit
        rts

Frame:
VBlank:
        rts

        section data,data

; 8 by 8 digits 0-9 and A-F.
Font:
        dc.b    %01111100,%11000110,%11001110,%11010110,%11100110,%11000110,%01111100,0
        dc.b    %00011000,%00111000,%00011000,%00011000,%00011000,%00011000,%01111110,0
        dc.b    %01111100,%11000110,%00000110,%00011100,%01110000,%11000000,%11111110,0
        dc.b    %01111100,%11000110,%00000110,%00111100,%00000110,%11000110,%01111100,0
        dc.b    %00011100,%00111100,%01101100,%11001100,%11111110,%00001100,%00001100,0
        dc.b    %11111110,%11000000,%11111100,%00000110,%00000110,%11000110,%01111100,0
        dc.b    %00111100,%01100000,%11000000,%11111100,%11000110,%11000110,%01111100,0
        dc.b    %11111110,%00000110,%00001100,%00011000,%00110000,%00110000,%00110000,0
        dc.b    %01111100,%11000110,%11000110,%01111100,%11000110,%11000110,%01111100,0
        dc.b    %01111100,%11000110,%11000110,%01111110,%00000110,%00001100,%01111000,0
        dc.b    %00111000,%01101100,%11000110,%11000110,%11111110,%11000110,%11000110,0
        dc.b    %11111100,%11000110,%11000110,%11111100,%11000110,%11000110,%11111100,0
        dc.b    %01111100,%11000110,%11000000,%11000000,%11000000,%11000110,%01111100,0
        dc.b    %11111000,%11001100,%11000110,%11000110,%11000110,%11001100,%11111000,0
        dc.b    %11111110,%11000000,%11000000,%11111000,%11000000,%11000000,%11111110,0
        dc.b    %11111110,%11000000,%11000000,%11111000,%11000000,%11000000,%11000000,0

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    COLOR00,$0123
        dc.w    COLOR01,$0eec
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    $ffff,$fffe

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
