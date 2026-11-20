; ch14_contention.s - the display taking the processor's turns at chip RAM. The copper list divides the screen
; into six zones of 36 lines, each with a different display: 1, 4, 5 and 6 low resolution bitplanes, then 2
; and 4 high resolution ones. All the way down, the processor does the same small job over and over (writing
; 256 bytes to chip RAM), changing the background colour each time, so each stripe is one go. Where the display
; takes cycles from the processor, the stripes get taller; each zone's label gives the average, in raster
; lines. Left mouse button to leave.
;
; How To Code v8, chapter 14. CC0: use it for anything.
;
; Made for PAL, as the zones go down to line 260.
;
; $VER: ch14_contention 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

ROWBYTES        equ     80              ; each line of the picture: room for a high resolution line
HEIGHT          equ     256
FIRSTLINE       equ     $2c             ; the first zone's first line
ZONELINES       equ     36
ZONES           equ     6
LINECLOCKS      equ     227             ; colour clocks in a PAL line
JOBBYTES        equ     256             ; what the job writes each time
TIMECOLUMN      equ     24              ; where each zone's time goes

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; Each zone starts by pointing bitplane 1 at its own part of the picture (so what it shows doesn't
        ; depend on how far the zones before it fetched), and the other five at a blank area. They're fetched
        ; just the same: what the display costs doesn't depend on what's in the bitplanes.
        lea     CopperZones,a0
        lea     Screen,a1
        moveq   #ZONES-1,d2
.zone:
        lea     ZONE_POINTERS(a0),a2
        move.l  a1,d0
        moveq   #6-1,d1
.pointer:
        swap    d0
        move.w  d0,2(a2)
        swap    d0
        move.w  d0,6(a2)
        addq.l  #8,a2
        move.l  #Blank,d0               ; bitplanes 2 to 6
        dbf     d1,.pointer
        lea     ZONE_SIZE(a0),a0
        lea     ROWBYTES*ZONELINES(a1),a1
        dbf     d2,.zone

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

        ; The zones' labels.
        lea     ZoneNames,a4
        moveq   #0,d4                   ; the zone
.name:
        move.l  (a4)+,a0
        moveq   #1,d0
        bsr     ZoneText
        lea     LinesText,a0
        moveq   #TIMECOLUMN+6,d0
        bsr     ZoneText
        addq.w  #1,d4
        cmp.w   #ZONES,d4
        bne.s   .name
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: from the first zone's first line, the job again and again until the last zone ends, each time
; in the other colour, timing each go. The times are added up for the zone it started in. Then one zone's
; average goes in its label: one a frame, as drawing all six would take the frame past its end on a 68000.

Frame:
        lea     ZoneTimes,a0
        moveq   #ZONES*2-1,d0
.clear:
        clr.l   (a0)+                   ; the times, then the counts
        dbf     d0,.clear

        move.w  #FIRSTLINE,d0
        bsr     WaitLine
        move.w  #$402,d6                ; the colour: d6 and d7 take turns
        move.w  #$024,d7
.go:
        bsr     BeamPosition            ; d0 = where, d1 = the line
        sub.l   #FIRSTLINE,d1
        divu    #ZONELINES,d1           ; the zone this go starts in
        cmp.w   #ZONES,d1
        bhs.s   .done
        move.w  d1,d5
        lsl.w   #2,d5                   ; a long for each zone
        move.l  d0,d4
        exg     d6,d7
        move.w  d6,COLOR00(a5)
        bsr     Job
        bsr     BeamPosition
        sub.l   d4,d0
        lea     ZoneTimes,a0
        add.l   d0,(a0,d5.w)
        addq.l  #1,ZONES*4(a0,d5.w)     ; ZoneCounts
        bra.s   .go
.done:
        move.w  #$000,COLOR00(a5)

        ; This frame's zone to label, in turn.
        move.w  LabelZone,d4
        addq.w  #1,d4
        cmp.w   #ZONES,d4
        blo.s   .label
        moveq   #0,d4
.label:
        move.w  d4,LabelZone
        move.w  d4,d5
        lsl.w   #2,d5
        lea     ZoneTimes,a0
        move.l  (a0,d5.w),d0
        move.l  ZONES*4(a0,d5.w),d1
        beq.s   .labelled               ; no go started in it (a very slow machine)
        divu    d1,d0                   ; the average, in colour clocks
        and.l   #$ffff,d0
        lea     TimeText,a0
        bsr     FormatLines
        lea     TimeText,a0
        moveq   #TIMECOLUMN,d0
        bsr     ZoneText
.labelled:
        rts

; Text in a zone's label (three lines down into it): a0 = the string, d0 = the column, d4 = the zone. The high
; resolution zones' text has each pixel doubled, so it's as wide as the rest.
ZoneText:
        move.w  d4,d1
        mulu    #ZONELINES,d1
        addq.w  #3,d1
        move.b  ZoneHires(pc,d4.w),d2   ; (not TST: on a 68000, TST can't read relative to the PC)
        bne     DrawTextHires
        bra     DrawText

ZoneHires:
        dc.b    0,0,0,0,1,1
        even

; The job: 64 longwords written to chip RAM, 16 to a loop. On an Amiga 500 the program itself is in chip or slow
; RAM too, so reading the instructions also takes turns on the chip bus.

Job:
        lea     JobBuffer,a0
        moveq   #0,d0
        moveq   #JOBBYTES/64-1,d1
.loop:
        rept    16
        move.l  d0,(a0)+
        endr
        dbf     d1,.loop
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits for a line (d0) to start. a5 = CUSTOM.

WaitLine:
.wait:
        move.l  VPOSR(a5),d1
        lsr.l   #8,d1
        and.w   #$1ff,d1
        cmp.w   d0,d1
        bne.s   .wait
        rts

; Where the beam is: d0 = line * 227 + position on the line, in colour clocks; d1 = the line.

BeamPosition:
        move.l  VPOSR(a5),d0
        moveq   #0,d2
        move.b  d0,d2
        lsr.l   #8,d0
        and.l   #$1ff,d0
        move.l  d0,d1
        mulu    #LINECLOCKS,d0
        add.l   d2,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Text, 8 by 8 pixels a character, into the picture. a0 = the string (capitals, figures and some signs, ending
; in zero), d0 = the column (0 to 39, in low resolution characters), d1 = the line.

DrawText:
        lea     Screen,a1
        mulu    #ROWBYTES,d1
        add.l   d1,a1
        add.w   d0,a1
.char:
        bsr     FindGlyph
        beq.s   .done
        moveq   #8-1,d1
        move.l  a1,a3
.row:
        move.b  (a2)+,(a3)
        lea     ROWBYTES(a3),a3
        dbf     d1,.row
        addq.l  #1,a1
        bra.s   .char
.done:
        rts

; The same for the high resolution zone: each pixel doubled, so the text is as wide as in the other zones.
DrawTextHires:
        lea     Screen,a1
        mulu    #ROWBYTES,d1
        add.l   d1,a1
        add.w   d0,a1
        add.w   d0,a1                   ; two bytes for each character
.char:
        bsr     FindGlyph
        beq.s   .done
        moveq   #8-1,d1
        move.l  a1,a3
.row:
        move.b  (a2)+,d2
        moveq   #0,d3
        move.b  d2,d3
        lsr.b   #4,d3                   ; the left four pixels...
        move.b  Doubled(pc,d3.w),(a3)   ; ...become eight
        and.w   #$f,d2
        move.b  Doubled(pc,d2.w),1(a3)
        lea     ROWBYTES(a3),a3
        dbf     d1,.row
        addq.l  #2,a1
        bra.s   .char
.done:
        rts

; Four pixels, each made two.
Doubled:
        dc.b    $00,$03,$0c,$0f,$30,$33,$3c,$3f,$c0,$c3,$cc,$cf,$f0,$f3,$fc,$ff

; The next character at a0: a2 = its picture in the font, or Z set at the string's end.
FindGlyph:
        moveq   #0,d2
        move.b  (a0)+,d2
        beq.s   .end
        sub.b   #32,d2
        and.w   #63,d2                  ; only the 64 characters from the space on
        lsl.w   #3,d2
        lea     Font,a2
        add.w   d2,a2
        moveq   #1,d2                   ; Z clear: there is a character
.end:
        rts

; A time in colour clocks (d0) as raster lines to a tenth, "nnn.n LINES", at a0.
FormatLines:
        move.l  d0,d1                   ; times 10
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

VBlank:
        rts

        section data,data

ZoneNames:
        dc.l    Name1,Name4,Name5,Name6,NameHires2,NameHires4
Name1:          dc.b    "1 BITPLANE",0
Name4:          dc.b    "4 BITPLANES",0
Name5:          dc.b    "5 BITPLANES",0
Name6:          dc.b    "6 BITPLANES",0
NameHires2:     dc.b    "2 HIRES PLANES",0
NameHires4:     dc.b    "4 HIRES PLANES",0
TimeText:       dc.b    "000.0",0
LinesText:      dc.b    "LINES",0
        even

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

ZoneTimes:      ds.l    ZONES           ; the colour clocks the goes in each zone took, added up
ZoneCounts:     ds.l    ZONES           ; and how many there were
LabelZone:      ds.w    1               ; the zone labelled last

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM. Each zone sets the display, then its bitplane pointers (filled in by Init).

        section copper,data_c

ZONE_POINTERS   equ     6*4+4           ; in each zone: a wait and six moves, then the pointers
ZONE_SIZE       equ     ZONE_POINTERS+12*4

ZONE            macro                   ; \1 = the first line, \2 = BPLCON0, \3 and \4 = DDFSTRT, DDFSTOP, \5 = modulo
        dc.w    (((\1-1)&$ff)<<8)!$df,$fffe
        dc.w    BPLCON0,\2
        dc.w    DDFSTRT,\3
        dc.w    DDFSTOP,\4
        dc.w    BPL1MOD,\5
        dc.w    BPL2MOD,\5
        dc.w    COLOR00,$000            ; (a spare move, so the pointers start where Init expects)
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
        dc.w    BPL5PTH,0,BPL5PTL,0
        dc.w    BPL6PTH,0,BPL6PTL,0
        endm

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; the usual PAL window, 320 by 256 (640 in high resolution)
        dc.w    DIWSTOP,$2cc1
        dc.w    BPLCON0,$0200
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    COLOR01,$0fff           ; the text, in bitplane 1 alone
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        ; Wait for the end of the line before each zone's first line, and set its display. A low resolution line
        ; fetches 40 bytes and skips the other 40 (modulo 40); a high resolution one fetches all 80.
CopperZones:
        ZONE    FIRSTLINE+0*ZONELINES,$1200,$0038,$00d0,40
        ZONE    FIRSTLINE+1*ZONELINES,$4200,$0038,$00d0,40
        ZONE    FIRSTLINE+2*ZONELINES,$5200,$0038,$00d0,40
        ZONE    FIRSTLINE+3*ZONELINES,$6200,$0038,$00d0,40
        ZONE    FIRSTLINE+4*ZONELINES,$a200,$003c,$00d4,0
        ZONE    FIRSTLINE+5*ZONELINES,$c200,$003c,$00d4,0
        ; The last zone ends below line 255: wait for the end of line 255 first, as a wait compares only
        ; the low eight bits of the line.
        dc.w    $ffdf,$fffe
        dc.w    (((FIRSTLINE+ZONES*ZONELINES-1)&$ff)<<8)!$df,$fffe
        dc.w    BPLCON0,$0200
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

JobBuffer:
        ds.b    JOBBYTES

        section screen,bss_c

Screen:         ds.b    ROWBYTES*HEIGHT ; bitplane 1: the labels
Blank:          ds.b    ROWBYTES*ZONELINES ; bitplanes 2 to 6, in every zone
