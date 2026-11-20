; ch13_dots.s - a ring of dots (a torus) turning in three dimensions: the simplest 3D object, and the first step
; to the filled cube in ch13_cube.s. A rotation matrix from three angles, perspective from a table instead of a
; divide, dots plotted by the processor and shaded by how near they are, two screens. Left mouse button to leave.
;
; How To Code v8, chapter 13. CC0: use it for anything. The maths is Carl-Henrik Skaarstedt's (Asterix of
; Movement), from his chapter in How To Code v7; the code is new.
;
; Build and run it with F9. Set SHOW_TIME to 1 to see how much of the frame it takes (the background turns red
; while the processor works).
;
; $VER: ch13_dots 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        ifnd    SHOW_TIME
SHOW_TIME       equ     0
        endc

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
PLANESIZE       equ     ROWBYTES*HEIGHT
PLANES          equ     2               ; three shades of dot
SCREENSIZE      equ     PLANESIZE*PLANES

RING            equ     16              ; dots round the ring...
TUBE            equ     6               ; ...and round the tube
POINTS          equ     RING*TUBE
TORUS_R         equ     70              ; from the centre to the middle of the tube
TUBE_R          equ     30              ; the tube's radius
DISTANCE        equ     320             ; from the eye to the centre (the screen is 256 from the eye)
CENTREX         equ     WIDTH/2
BAND            equ     30              ; nearer than -BAND: bright; further than BAND: dim
BACKGROUND      equ     $012

; Each screen's information: where it is, and where its dots were drawn, to rub them out next time.
SCR_PLANES      equ     0               ; long: the first bitplane (the second follows it)
SCR_DOTS        equ     4               ; long: a list of POINTS words, each dot's byte in the bitplane

; MULF: multiplies a word by a 2.14 fixed point number (a sine, or a matrix entry) and keeps the whole part.
; MULS gives 32 bits, 14 of them fraction; two doublings and a swap leave bits 14 to 29 in the low word: the
; same as shifting right by 14 (asr.l #8 then asr.l #6, 44 cycles), in 20.
MULF    macro
        muls    \1,\2
        add.l   \2,\2
        add.l   \2,\2
        swap    \2
        endm

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; All eight sprites at an empty sprite, so the system's mouse pointer can't show.
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

        ; NTSC shows 200 lines: a shorter window, and the ring higher up.
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
        move.w  #100,CentreY
.pal:

        ; The torus. Round the ring at angle u, round the tube at angle v:
        ;   x = (TORUS_R + TUBE_R * cos v) * cos u
        ;   y = (TORUS_R + TUBE_R * cos v) * sin u
        ;   z = TUBE_R * sin v
        lea     Points,a0
        lea     Sine,a1
        moveq   #0,d6                   ; u, as a byte offset into Sine
        moveq   #RING-1,d7
.round:
        moveq   #0,d5                   ; v, as a byte offset times 65536 (1024 doesn't divide by 6)
        moveq   #TUBE-1,d4
.tube:
        move.l  d5,d3
        swap    d3
        and.w   #$7fe,d3                ; d3 = v, to the word below
        move.w  d3,d0                   ; d1 = TORUS_R + TUBE_R * cos v
        add.w   #512,d0
        and.w   #$7fe,d0
        move.w  (a1,d0.w),d1
        MULF    #TUBE_R,d1
        add.w   #TORUS_R,d1
        move.w  d6,d0                   ; x
        add.w   #512,d0
        and.w   #$7fe,d0
        move.w  (a1,d0.w),d2
        MULF    d1,d2
        move.w  d2,(a0)+
        move.w  (a1,d6.w),d2            ; y
        MULF    d1,d2
        move.w  d2,(a0)+
        move.w  (a1,d3.w),d2            ; z
        MULF    #TUBE_R,d2
        move.w  d2,(a0)+
        add.l   #(2048<<16)/TUBE,d5
        dbf     d4,.tube
        add.w   #2048/RING,d6
        dbf     d7,.round

        ; Perspective without a divide: a table of 256 * 16384 / (z + DISTANCE), for z from -128 to 127. Then
        ; x * 256 / (z + DISTANCE) is a MULF by the table's entry: a multiply (up to 70 cycles) instead of a
        ; divide (up to 158). The divides are done here, once.
        lea     Recip,a0
        move.w  #DISTANCE-128,d1
        move.w  #256-1,d7
.recip:
        move.l  #256*16384,d0
        divu    d1,d0
        move.w  d0,(a0)+
        addq.w  #1,d1
        dbf     d7,.recip

        ; Where each line of the screen starts: a table, to save a multiply for every dot.
        lea     RowOffsets,a0
        moveq   #0,d0
        move.w  #HEIGHT-1,d7
.row:
        move.w  d0,(a0)+
        add.w   #ROWBYTES,d0
        dbf     d7,.row

        ; Both screens cleared, by the blitter: D only, minterm 0. Each is two bitplanes, 512 lines of 20 words.
        BLIT_BEGIN
        move.l  #$01000000,BLTCON0(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  #Screen1,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        jsr     WaitBlitter
        move.l  #Screen2,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        BLIT_END

        ; Screen 1 (empty) on show from the first frame; screen 2 is drawn first.
        move.l  #Screen1,PendingScreen
        st      SwapPending
        bsr     VBlank
        move.l  #ScreenInfo2,DrawScreen
        move.l  #ScreenInfo1,ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the next picture, into the screen not on show.

Frame:
        ; Don't draw into a screen until it has gone from the display (see ch13_cube.s: with the system
        ; running, Frame can be called again before the last picture has been put on show).
.waitswap:
        tst.b   SwapPending
        beq.s   .swapped
        jsr     WaitFrame
        bra.s   .waitswap
.swapped:

        ifne    SHOW_TIME
        move.w  #$a00,COLOR00(a5)
        endc

        lea     Angles,a0
        add.w   #2*2,(a0)+
        add.w   #2*3,(a0)+
        add.w   #2*5,(a0)+

        bsr     CalcMatrix
        move.l  DrawScreen,a4

        ; Rub out the dots drawn in this screen last time: just the bytes they were in, in both bitplanes. Far
        ; quicker than clearing the screen, for a hundred dots.
        move.l  SCR_PLANES(a4),a6
        move.l  SCR_DOTS(a4),a3
        moveq   #POINTS-1,d7
.erase:
        move.w  (a3)+,d0
        lea     (a6,d0.w),a0
        clr.b   (a0)
        clr.b   PLANESIZE(a0)
        dbf     d7,.erase

        bsr     DrawDots

        ; Ready: the vertical blank puts this screen on show, and the other one is drawn into next.
        move.l  SCR_PLANES(a4),PendingScreen
        st      SwapPending
        move.l  ShowScreen,DrawScreen
        move.l  a4,ShowScreen

        ifne    SHOW_TIME
        move.w  #BACKGROUND,COLOR00(a5)
        endc
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt: if a new picture is ready, its bitplanes go into the copper
; list. The copper sets them at line $28, after this has run and before the display starts.

VBlank:
        tst.b   SwapPending
        beq.s   .none
        lea     CopperPlanes,a0
        move.l  PendingScreen,d0
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        move.l  PendingScreen,d0
        add.l   #PLANESIZE,d0
        move.w  d0,14(a0)
        swap    d0
        move.w  d0,10(a0)
        sf      SwapPending
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The rotation matrix from the three angles: Carl-Henrik's way, working out c1*s2 and s1*s2 once and using each
; twice. It turns about z (angle 1), then y (angle 2), then x (angle 3):
;
;   xx = c2*c1                  xy = -c2*s1                 xz = -s2
;   yx = s1*c3 - c1*s2*s3       yy = c1*c3 + s1*s2*s3       yz = -c2*s3
;   zx = s1*s3 + c1*s2*c3       zy = c1*s3 - s1*s2*c3       zz = c2*c3
;
; 14 multiplies, once a frame; then each point takes 9.

SC_C1   equ     0
SC_S1   equ     2
SC_C2   equ     4
SC_S2   equ     6
SC_C3   equ     8
SC_S3   equ     10

CalcMatrix:
        ; The sine and cosine of each angle. cos(a) = sin(a + a quarter turn): 256 words on, 512 bytes.
        lea     Sine,a0
        lea     Angles,a1
        lea     SinCos,a2
        moveq   #3-1,d7
.angle:
        move.w  (a1)+,d0
        and.w   #$7fe,d0                ; the angle wraps round: 1024 words
        move.w  (a0,d0.w),SC_S1(a2)
        add.w   #512,d0
        and.w   #$7fe,d0
        move.w  (a0,d0.w),SC_C1(a2)
        addq.l  #4,a2
        dbf     d7,.angle

        lea     SinCos,a0
        lea     Matrix,a1
        move.w  SC_C1(a0),d6            ; d6 = c1*s2
        MULF    SC_S2(a0),d6
        move.w  SC_S1(a0),d7            ; d7 = s1*s2
        MULF    SC_S2(a0),d7

        move.w  SC_C2(a0),d0            ; xx = c2*c1
        MULF    SC_C1(a0),d0
        move.w  d0,(a1)
        move.w  SC_C2(a0),d0            ; xy = -c2*s1
        MULF    SC_S1(a0),d0
        neg.w   d0
        move.w  d0,2(a1)
        move.w  SC_S2(a0),d0            ; xz = -s2
        neg.w   d0
        move.w  d0,4(a1)

        move.w  SC_S1(a0),d0            ; yx = s1*c3 - c1*s2*s3
        MULF    SC_C3(a0),d0
        move.w  d6,d1
        MULF    SC_S3(a0),d1
        sub.w   d1,d0
        move.w  d0,6(a1)
        move.w  SC_C1(a0),d0            ; yy = c1*c3 + s1*s2*s3
        MULF    SC_C3(a0),d0
        move.w  d7,d1
        MULF    SC_S3(a0),d1
        add.w   d1,d0
        move.w  d0,8(a1)
        move.w  SC_C2(a0),d0            ; yz = -c2*s3
        MULF    SC_S3(a0),d0
        neg.w   d0
        move.w  d0,10(a1)

        move.w  SC_S1(a0),d0            ; zx = s1*s3 + c1*s2*c3
        MULF    SC_S3(a0),d0
        move.w  d6,d1
        MULF    SC_C3(a0),d1
        add.w   d1,d0
        move.w  d0,12(a1)
        move.w  SC_C1(a0),d0            ; zy = c1*s3 - s1*s2*c3
        MULF    SC_S3(a0),d0
        move.w  d7,d1
        MULF    SC_C3(a0),d1
        sub.w   d1,d0
        move.w  d0,14(a1)
        move.w  SC_C2(a0),d0            ; zz = c2*c3
        MULF    SC_C3(a0),d0
        move.w  d0,16(a1)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Every dot: turned by the matrix, put in perspective, and plotted. a4 = the screen's information.
;
; For each dot the processor does 9 multiplies to turn it and 2 for perspective: about 1000 cycles in all, so a
; 68000 can turn and plot about a hundred dots a frame and still have time for something else.

DrawDots:
        lea     Points,a0
        lea     Matrix,a1
        lea     Recip+128*2,a2          ; the middle of the table: z = 0
        lea     RowOffsets,a3
        move.l  SCR_PLANES(a4),a6
        move.l  SCR_DOTS(a4),a5         ; (a5 isn't CUSTOM in here)
        moveq   #POINTS-1,d7
.dot:
        movem.w (a0)+,d0-d2             ; x, y, z

        move.w  d0,d3                   ; z first, for the perspective
        muls    12(a1),d3
        move.w  d1,d4
        muls    14(a1),d4
        add.l   d4,d3
        move.w  d2,d4
        muls    16(a1),d4
        add.l   d4,d3
        add.l   d3,d3
        add.l   d3,d3
        swap    d3
        move.w  d3,d6                   ; d6 = z, for the shade
        add.w   d3,d3
        move.w  (a2,d3.w),d5            ; d5 = 256 * 16384 / (z + DISTANCE)

        move.w  d0,d3                   ; x
        muls    (a1),d3
        move.w  d1,d4
        muls    2(a1),d4
        add.l   d4,d3
        move.w  d2,d4
        muls    4(a1),d4
        add.l   d4,d3
        add.l   d3,d3
        add.l   d3,d3
        swap    d3
        MULF    d5,d3                   ; x * 256 / (z + DISTANCE)
        add.w   #CENTREX,d3

        move.w  d0,d4                   ; y
        muls    6(a1),d4
        move.w  d1,d0
        muls    8(a1),d0
        add.l   d0,d4
        move.w  d2,d0
        muls    10(a1),d0
        add.l   d0,d4
        add.l   d4,d4
        add.l   d4,d4
        swap    d4
        MULF    d5,d4
        add.w   CentreY,d4

        ; The byte: the line's start from the table, plus x / 8. The bit: 7 - (x and 7), as bset on memory
        ; numbers a byte's bits from the right and uses only the lowest three bits of the number.
        add.w   d4,d4
        move.w  (a3,d4.w),d4
        move.w  d3,d0
        lsr.w   #3,d0
        add.w   d0,d4
        not.w   d3
        move.w  d4,(a5)+                ; noted, to rub out next time

        ; The shade: near dots in colour 3 (both bitplanes), far ones in colour 1 (the first), the rest in
        ; colour 2 (the second).
        cmp.w   #-BAND,d6
        blt.s   .near
        cmp.w   #BAND,d6
        bgt.s   .far
        add.w   #PLANESIZE,d4
        bset    d3,(a6,d4.w)
        bra.s   .next
.near:
        bset    d3,(a6,d4.w)
        add.w   #PLANESIZE,d4
.far:
        bset    d3,(a6,d4.w)
.next:
        dbf     d7,.dot
        lea     CUSTOM,a5
        rts

        section data,data

Angles:         dc.w    0,0,0           ; as byte offsets into Sine
SinCos:         ds.w    6
Matrix:         ds.w    9               ; xx xy xz yx yy yz zx zy zz, 2.14 fixed point
CentreY:        dc.w    HEIGHT/2
DrawScreen:     dc.l    0               ; the screen being drawn in
ShowScreen:     dc.l    0               ; the screen on show (or about to be)
PendingScreen:  dc.l    0               ; from Frame to VBlank: the screen to show next...
SwapPending:    dc.b    0               ; ...and that it's ready
        even

ScreenInfo1:    dc.l    Screen1,Dots1
ScreenInfo2:    dc.l    Screen2,Dots2

; A whole turn of sine in 1024 words: round(sin(2*pi*i/1024) * 16384), 2.14 fixed point. Made by a script;
; Limoniter's table generator (Tools, General, Table generator) makes the same: a sine, 1024 signed words,
; amplitude 16384.
Sine:
        dc.w    0,101,201,302,402,503,603,704,804,904,1005,1105,1205,1306,1406,1506
        dc.w    1606,1706,1806,1906,2006,2105,2205,2305,2404,2503,2603,2702,2801,2900,2999,3098
        dc.w    3196,3295,3393,3492,3590,3688,3786,3883,3981,4078,4176,4273,4370,4467,4563,4660
        dc.w    4756,4852,4948,5044,5139,5235,5330,5425,5520,5614,5708,5803,5897,5990,6084,6177
        dc.w    6270,6363,6455,6547,6639,6731,6823,6914,7005,7096,7186,7276,7366,7456,7545,7635
        dc.w    7723,7812,7900,7988,8076,8163,8250,8337,8423,8509,8595,8680,8765,8850,8935,9019
        dc.w    9102,9186,9269,9352,9434,9516,9598,9679,9760,9841,9921,10001,10080,10159,10238,10316
        dc.w    10394,10471,10549,10625,10702,10778,10853,10928,11003,11077,11151,11224,11297,11370,11442,11514
        dc.w    11585,11656,11727,11797,11866,11935,12004,12072,12140,12207,12274,12340,12406,12472,12537,12601
        dc.w    12665,12729,12792,12854,12916,12978,13039,13100,13160,13219,13279,13337,13395,13453,13510,13567
        dc.w    13623,13678,13733,13788,13842,13896,13949,14001,14053,14104,14155,14206,14256,14305,14354,14402
        dc.w    14449,14497,14543,14589,14635,14680,14724,14768,14811,14854,14896,14937,14978,15019,15059,15098
        dc.w    15137,15175,15213,15250,15286,15322,15357,15392,15426,15460,15493,15525,15557,15588,15619,15649
        dc.w    15679,15707,15736,15763,15791,15817,15843,15868,15893,15917,15941,15964,15986,16008,16029,16049
        dc.w    16069,16088,16107,16125,16143,16160,16176,16192,16207,16221,16235,16248,16261,16273,16284,16295
        dc.w    16305,16315,16324,16332,16340,16347,16353,16359,16364,16369,16373,16376,16379,16381,16383,16384
        dc.w    16384,16384,16383,16381,16379,16376,16373,16369,16364,16359,16353,16347,16340,16332,16324,16315
        dc.w    16305,16295,16284,16273,16261,16248,16235,16221,16207,16192,16176,16160,16143,16125,16107,16088
        dc.w    16069,16049,16029,16008,15986,15964,15941,15917,15893,15868,15843,15817,15791,15763,15736,15707
        dc.w    15679,15649,15619,15588,15557,15525,15493,15460,15426,15392,15357,15322,15286,15250,15213,15175
        dc.w    15137,15098,15059,15019,14978,14937,14896,14854,14811,14768,14724,14680,14635,14589,14543,14497
        dc.w    14449,14402,14354,14305,14256,14206,14155,14104,14053,14001,13949,13896,13842,13788,13733,13678
        dc.w    13623,13567,13510,13453,13395,13337,13279,13219,13160,13100,13039,12978,12916,12854,12792,12729
        dc.w    12665,12601,12537,12472,12406,12340,12274,12207,12140,12072,12004,11935,11866,11797,11727,11656
        dc.w    11585,11514,11442,11370,11297,11224,11151,11077,11003,10928,10853,10778,10702,10625,10549,10471
        dc.w    10394,10316,10238,10159,10080,10001,9921,9841,9760,9679,9598,9516,9434,9352,9269,9186
        dc.w    9102,9019,8935,8850,8765,8680,8595,8509,8423,8337,8250,8163,8076,7988,7900,7812
        dc.w    7723,7635,7545,7456,7366,7276,7186,7096,7005,6914,6823,6731,6639,6547,6455,6363
        dc.w    6270,6177,6084,5990,5897,5803,5708,5614,5520,5425,5330,5235,5139,5044,4948,4852
        dc.w    4756,4660,4563,4467,4370,4273,4176,4078,3981,3883,3786,3688,3590,3492,3393,3295
        dc.w    3196,3098,2999,2900,2801,2702,2603,2503,2404,2305,2205,2105,2006,1906,1806,1706
        dc.w    1606,1506,1406,1306,1205,1105,1005,904,804,704,603,503,402,302,201,101
        dc.w    0,-101,-201,-302,-402,-503,-603,-704,-804,-904,-1005,-1105,-1205,-1306,-1406,-1506
        dc.w    -1606,-1706,-1806,-1906,-2006,-2105,-2205,-2305,-2404,-2503,-2603,-2702,-2801,-2900,-2999,-3098
        dc.w    -3196,-3295,-3393,-3492,-3590,-3688,-3786,-3883,-3981,-4078,-4176,-4273,-4370,-4467,-4563,-4660
        dc.w    -4756,-4852,-4948,-5044,-5139,-5235,-5330,-5425,-5520,-5614,-5708,-5803,-5897,-5990,-6084,-6177
        dc.w    -6270,-6363,-6455,-6547,-6639,-6731,-6823,-6914,-7005,-7096,-7186,-7276,-7366,-7456,-7545,-7635
        dc.w    -7723,-7812,-7900,-7988,-8076,-8163,-8250,-8337,-8423,-8509,-8595,-8680,-8765,-8850,-8935,-9019
        dc.w    -9102,-9186,-9269,-9352,-9434,-9516,-9598,-9679,-9760,-9841,-9921,-10001,-10080,-10159,-10238,-10316
        dc.w    -10394,-10471,-10549,-10625,-10702,-10778,-10853,-10928,-11003,-11077,-11151,-11224,-11297,-11370,-11442,-11514
        dc.w    -11585,-11656,-11727,-11797,-11866,-11935,-12004,-12072,-12140,-12207,-12274,-12340,-12406,-12472,-12537,-12601
        dc.w    -12665,-12729,-12792,-12854,-12916,-12978,-13039,-13100,-13160,-13219,-13279,-13337,-13395,-13453,-13510,-13567
        dc.w    -13623,-13678,-13733,-13788,-13842,-13896,-13949,-14001,-14053,-14104,-14155,-14206,-14256,-14305,-14354,-14402
        dc.w    -14449,-14497,-14543,-14589,-14635,-14680,-14724,-14768,-14811,-14854,-14896,-14937,-14978,-15019,-15059,-15098
        dc.w    -15137,-15175,-15213,-15250,-15286,-15322,-15357,-15392,-15426,-15460,-15493,-15525,-15557,-15588,-15619,-15649
        dc.w    -15679,-15707,-15736,-15763,-15791,-15817,-15843,-15868,-15893,-15917,-15941,-15964,-15986,-16008,-16029,-16049
        dc.w    -16069,-16088,-16107,-16125,-16143,-16160,-16176,-16192,-16207,-16221,-16235,-16248,-16261,-16273,-16284,-16295
        dc.w    -16305,-16315,-16324,-16332,-16340,-16347,-16353,-16359,-16364,-16369,-16373,-16376,-16379,-16381,-16383,-16384
        dc.w    -16384,-16384,-16383,-16381,-16379,-16376,-16373,-16369,-16364,-16359,-16353,-16347,-16340,-16332,-16324,-16315
        dc.w    -16305,-16295,-16284,-16273,-16261,-16248,-16235,-16221,-16207,-16192,-16176,-16160,-16143,-16125,-16107,-16088
        dc.w    -16069,-16049,-16029,-16008,-15986,-15964,-15941,-15917,-15893,-15868,-15843,-15817,-15791,-15763,-15736,-15707
        dc.w    -15679,-15649,-15619,-15588,-15557,-15525,-15493,-15460,-15426,-15392,-15357,-15322,-15286,-15250,-15213,-15175
        dc.w    -15137,-15098,-15059,-15019,-14978,-14937,-14896,-14854,-14811,-14768,-14724,-14680,-14635,-14589,-14543,-14497
        dc.w    -14449,-14402,-14354,-14305,-14256,-14206,-14155,-14104,-14053,-14001,-13949,-13896,-13842,-13788,-13733,-13678
        dc.w    -13623,-13567,-13510,-13453,-13395,-13337,-13279,-13219,-13160,-13100,-13039,-12978,-12916,-12854,-12792,-12729
        dc.w    -12665,-12601,-12537,-12472,-12406,-12340,-12274,-12207,-12140,-12072,-12004,-11935,-11866,-11797,-11727,-11656
        dc.w    -11585,-11514,-11442,-11370,-11297,-11224,-11151,-11077,-11003,-10928,-10853,-10778,-10702,-10625,-10549,-10471
        dc.w    -10394,-10316,-10238,-10159,-10080,-10001,-9921,-9841,-9760,-9679,-9598,-9516,-9434,-9352,-9269,-9186
        dc.w    -9102,-9019,-8935,-8850,-8765,-8680,-8595,-8509,-8423,-8337,-8250,-8163,-8076,-7988,-7900,-7812
        dc.w    -7723,-7635,-7545,-7456,-7366,-7276,-7186,-7096,-7005,-6914,-6823,-6731,-6639,-6547,-6455,-6363
        dc.w    -6270,-6177,-6084,-5990,-5897,-5803,-5708,-5614,-5520,-5425,-5330,-5235,-5139,-5044,-4948,-4852
        dc.w    -4756,-4660,-4563,-4467,-4370,-4273,-4176,-4078,-3981,-3883,-3786,-3688,-3590,-3492,-3393,-3295
        dc.w    -3196,-3098,-2999,-2900,-2801,-2702,-2603,-2503,-2404,-2305,-2205,-2105,-2006,-1906,-1806,-1706
        dc.w    -1606,-1506,-1406,-1306,-1205,-1105,-1005,-904,-804,-704,-603,-503,-402,-302,-201,-101


; ---------------------------------------------------------------------------------------------------------------

        section bss,bss

Points:         ds.w    POINTS*3        ; the torus, made by Init
Recip:          ds.w    256             ; 256 * 16384 / (z + DISTANCE), z from -128 to 127
RowOffsets:     ds.w    HEIGHT
Dots1:          ds.w    POINTS          ; where each screen's dots were drawn
Dots2:          ds.w    POINTS

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
        dc.w    BPLCON0,$2200           ; two bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,BACKGROUND
        dc.w    COLOR01,$358            ; far
        dc.w    COLOR02,$69c            ; middle
        dc.w    COLOR03,$eff            ; near
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    $2801,$fffe             ; the screen is set at line $28, after VBlank has changed it
CopperPlanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screens,bss_c

Screen1:
        ds.b    SCREENSIZE
Screen2:
        ds.b    SCREENSIZE
