; A starting point for an Atari ST demo, for any ST, Mega ST, STE or Mega STE (or a Falcon in ST low resolution).
;
; It goes into supervisor mode, keeps what it changes (the resolution, the screen's address, the palette, the
; MFP's interrupt settings and the vectors it takes over) and gives the machine back as it found it at the end.
; In between it shows one low-resolution screen of coloured columns, with raster bars moving behind them: the
; VBL handler at $70 starts Timer B counting display lines, and Timer B's handler at $120 changes colour 0 every
; few lines. Press space to leave.
;
; Devpac/vasm syntax. To make a program TOS runs:
;   vasmm68k_mot -Ftos -nosym -o demo.tos demo.s
; Everything it needs is defined here, so no include files are needed.

; ---------------------------------------------------------------------------------------------------------------
; The hardware, as short addresses: $FFFF8240.W is $FF8240, reached with a 16-bit address.

COLOR0          equ     $ffff8240       ; the 16 colours, a word each
IERA            equ     $fffffa07       ; MFP: interrupt enable A (bit 0 Timer B)
IERB            equ     $fffffa09       ; and B (bit 6 the keyboard and MIDI, bit 5 Timer C)
ISRA            equ     $fffffa0f       ; MFP: in service A
IMRA            equ     $fffffa13       ; MFP: interrupt mask A
IMRB            equ     $fffffa15       ; and B
VR              equ     $fffffa17       ; MFP: vector register
TBCR            equ     $fffffa1b       ; Timer B control: 8 counts display lines
TBDR            equ     $fffffa21       ; Timer B data: how many lines between interrupts
KEYDATA         equ     $fffffc02       ; the last byte from the keyboard

VBL_VECTOR      equ     $70
TIMERB_VECTOR   equ     $120

BAR_LINES       equ     8               ; lines between colour changes
SPACE           equ     $39             ; the space bar's scan code

; ---------------------------------------------------------------------------------------------------------------

start:
        ; TOS gives a program all the free memory. Keep only what the program needs, so Malloc has some to give.
        move.l  4(sp),a5                ; the program's basepage
        lea     stack_top(pc),sp        ; a stack of its own, inside the program
        move.l  $0c(a5),d0              ; the length of its code,
        add.l   $14(a5),d0              ; its data
        add.l   $1c(a5),d0              ; and its BSS,
        add.l   #$100,d0                ; and the basepage
        move.l  d0,-(sp)
        move.l  a5,-(sp)
        clr.w   -(sp)
        move.w  #$4a,-(sp)              ; Mshrink
        trap    #1
        lea     12(sp),sp

        ; Supervisor mode: the hardware's registers can only be reached from it.
        clr.l   -(sp)
        move.w  #$20,-(sp)              ; Super
        trap    #1
        addq.l  #6,sp
        move.l  d0,old_stack

        ; Memory for the screen: 32000 bytes, starting on a multiple of 256 as the ST's video needs.
        move.l  #32000+255,-(sp)
        move.w  #$48,-(sp)              ; Malloc
        trap    #1
        addq.l  #6,sp
        tst.l   d0
        beq     no_memory
        move.l  d0,screen_block
        add.l   #255,d0
        clr.b   d0
        move.l  d0,screen

        ; What to put back at the end: the resolution, the two screens and the palette.
        move.w  #4,-(sp)                ; Getrez
        trap    #14
        addq.l  #2,sp
        move.w  d0,old_rez
        move.w  #2,-(sp)                ; Physbase
        trap    #14
        addq.l  #2,sp
        move.l  d0,old_physbase
        move.w  #3,-(sp)                ; Logbase
        trap    #14
        addq.l  #2,sp
        move.l  d0,old_logbase
        movem.l COLOR0.w,d0-d7
        movem.l d0-d7,old_palette

        ; Low resolution, on the new screen, from the next frame.
        clr.w   -(sp)                   ; low resolution
        move.l  screen(pc),-(sp)        ; the screen shown
        move.l  screen(pc),-(sp)        ; the screen drawn on
        move.w  #5,-(sp)                ; Setscreen
        trap    #14
        lea     12(sp),sp
        move.w  #37,-(sp)               ; Vsync: wait for it to happen
        trap    #14
        addq.l  #2,sp

        bsr     draw_screen
        movem.l palette(pc),d0-d7
        movem.l d0-d7,COLOR0.w

        ; Take over the interrupts: none at all while they're changed.
        move.w  #$2700,sr
        lea     saved_mfp(pc),a0
        move.b  IERA.w,(a0)+
        move.b  IERB.w,(a0)+
        move.b  IMRA.w,(a0)+
        move.b  IMRB.w,(a0)+
        move.b  TBCR.w,(a0)+
        move.b  TBDR.w,(a0)+
        move.b  VR.w,(a0)+
        move.l  VBL_VECTOR.w,old_vbl
        move.l  TIMERB_VECTOR.w,old_timer_b

        move.l  #vbl,VBL_VECTOR.w
        move.l  #timer_b,TIMERB_VECTOR.w
        clr.b   TBCR.w                  ; Timer B stopped until the first VBL starts it
        move.b  #%00000001,IERA.w       ; of channels 8-15, only Timer B
        move.b  #%00000001,IMRA.w
        move.b  #%01000000,IERB.w       ; of channels 0-7, only the keyboard, so keys still come in
        move.b  #%01000000,IMRB.w
        bset    #3,VR.w                 ; software end of interrupt: handlers clear their in-service bit
        move.w  #$2300,sr               ; interrupts on again

; ---------------------------------------------------------------------------------------------------------------
; The main loop: once a frame, until space is pressed.

main_loop:
        move.w  frames(pc),d0
.wait:  cmp.w   frames(pc),d0           ; wait for the VBL handler to count the next frame
        beq.s   .wait

        ; Work to do each frame goes here.

        cmp.b   #SPACE,KEYDATA.w        ; space pressed?
        bne.s   main_loop

; ---------------------------------------------------------------------------------------------------------------
; Everything back as it was.

        move.w  #$2700,sr
        clr.b   TBCR.w
        lea     saved_mfp(pc),a0
        move.b  (a0)+,IERA.w
        move.b  (a0)+,IERB.w
        move.b  (a0)+,IMRA.w
        move.b  (a0)+,IMRB.w
        move.b  1(a0),TBDR.w            ; the count before the control, which starts it
        move.b  (a0)+,TBCR.w
        addq.l  #1,a0
        move.b  (a0)+,VR.w
        move.l  old_vbl(pc),VBL_VECTOR.w
        move.l  old_timer_b(pc),TIMERB_VECTOR.w
        move.w  #$2300,sr

        movem.l old_palette(pc),d0-d7
        movem.l d0-d7,COLOR0.w
        move.w  old_rez(pc),-(sp)
        move.l  old_physbase(pc),-(sp)
        move.l  old_logbase(pc),-(sp)
        move.w  #5,-(sp)                ; Setscreen
        trap    #14
        lea     12(sp),sp
        move.w  #37,-(sp)               ; Vsync
        trap    #14
        addq.l  #2,sp

        ; Eat the keys pressed meanwhile, so the space doesn't reach the desktop.
flush_keys:
        move.w  #$0b,-(sp)              ; Cconis: is a key waiting?
        trap    #1
        addq.l  #2,sp
        tst.w   d0
        beq.s   keys_flushed
        move.w  #7,-(sp)                ; Crawcin: take it
        trap    #1
        addq.l  #2,sp
        bra.s   flush_keys
keys_flushed:

        move.l  screen_block(pc),-(sp)
        move.w  #$49,-(sp)              ; Mfree: the screen's memory
        trap    #1
        addq.l  #6,sp

no_memory:
        move.l  old_stack(pc),-(sp)
        move.w  #$20,-(sp)              ; Super: back to user mode
        trap    #1
        addq.l  #6,sp
        clr.w   -(sp)                   ; Pterm0: back to the desktop
        trap    #1

; ---------------------------------------------------------------------------------------------------------------
; The VBL handler, at the start of each frame: Timer B is started counting lines afresh, and the bars move on.

vbl:
        movem.l d0/a0,-(sp)
        clr.b   TBCR.w                  ; stop Timer B
        move.b  #BAR_LINES,TBDR.w       ; an interrupt every BAR_LINES lines
        move.b  #8,TBCR.w               ; event count mode: it counts display lines
        move.w  frames(pc),d0
        addq.w  #1,d0
        move.w  d0,frames
        and.w   #31,d0                  ; where in the colour table this frame's bars start
        add.w   d0,d0
        lea     bar_colours(pc),a0
        add.w   d0,a0
        move.w  (a0)+,COLOR0.w          ; the colour above the first change
        move.l  a0,bar_pointer
        movem.l (sp)+,d0/a0
        rte

; Timer B's handler, every BAR_LINES lines down the picture: colour 0 becomes the next in the table.

timer_b:
        move.l  a0,-(sp)
        move.l  bar_pointer(pc),a0
        move.w  (a0)+,COLOR0.w
        move.l  a0,bar_pointer
        move.l  (sp)+,a0
        bclr    #0,ISRA.w               ; done: clear Timer B's in-service bit
        rte

; ---------------------------------------------------------------------------------------------------------------
; The picture: columns of colours 1 to 15, 16 pixels wide, in a band across the middle. In low resolution each
; 16 pixels are four words, one a bitplane, and a pixel's colour is its bits in the four.

draw_screen:
        move.l  screen(pc),a0
        move.w  #32000/4-1,d0
.clear: clr.l   (a0)+
        dbra    d0,.clear
        move.l  screen(pc),a0
        lea     60*160(a0),a0           ; from line 60
        move.w  #80-1,d1                ; for 80 lines
.line:  moveq   #0,d2                   ; the 20 columns across
.column:
        move.w  d2,d3
        add.w   d1,d3                   ; a step every line, so the columns lean
        lsr.w   #3,d3
        add.w   d2,d3
        and.w   #15,d3
        bne.s   .colour
        moveq   #1,d3                   ; colour 0 is the bars': not in the columns
.colour:
        moveq   #4-1,d4
.plane: lsr.w   #1,d3                   ; each plane takes the next bit of the colour
        scs     d5
        ext.w   d5
        move.w  d5,(a0)+
        dbra    d4,.plane
        addq.w  #1,d2
        cmp.w   #20,d2
        bne.s   .column
        dbra    d1,.line
        rts

; ---------------------------------------------------------------------------------------------------------------

palette:
        dc.w    $000,$700,$730,$750,$770,$470,$070,$073
        dc.w    $077,$047,$007,$407,$707,$704,$777,$444

; Colour 0 down the screen: two rounds of a gradient, so 32 starting places all have 26 colours after them.
bar_colours:
        rept    2
        dc.w    $100,$200,$300,$400,$500,$600,$700,$710,$720,$730,$740,$750,$760,$770,$670,$570
        dc.w    $470,$370,$270,$170,$070,$071,$072,$073,$074,$075,$076,$077,$067,$057,$047,$037
        endr

        even
frames:         dc.w    0
old_rez:        dc.w    0
bar_pointer:    dc.l    0
screen:         dc.l    0
screen_block:   dc.l    0
old_stack:      dc.l    0
old_physbase:   dc.l    0
old_logbase:    dc.l    0
old_vbl:        dc.l    0
old_timer_b:    dc.l    0
old_palette:    ds.w    16
saved_mfp:      ds.b    8

        even
        ds.b    4096                    ; the stack
stack_top:
