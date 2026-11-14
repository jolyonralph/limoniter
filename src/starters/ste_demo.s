; A starting point for an Atari STE demo, for the STE and Mega STE: the ST starter, with what the STE adds.
;
; Like the ST one, it goes into supervisor mode, keeps what it changes and gives the machine back as it found it,
; with raster bars from Timer B behind a picture. The STE's own: it checks it's running on one (the _MCH cookie),
; its colours have 16 levels a gun rather than 8 (the extra bit is the lowest, kept above the other three, so
; level 1 is $8 and level 15 is $F), and the picture is twice as wide as the screen, moved smoothly left and
; right by the STE's hardware scrolling: the screen's address to the word ($FF820D), the pixels within the word
; ($FF8265), and the words skipped at the end of each line ($FF820F). Press space to leave.
;
; Devpac/vasm syntax. To make a program TOS runs:
;   vasmm68k_mot -Ftos -nosym -o demo.tos demo.s
; Everything it needs is defined here, so no include files are needed.

; ---------------------------------------------------------------------------------------------------------------
; The hardware, as short addresses: $FFFF8240.W is $FF8240, reached with a 16-bit address.

DBASEH          equ     $ffff8201       ; the screen's address: top byte,
DBASEM          equ     $ffff8203       ; middle byte
DBASEL          equ     $ffff820d       ; and (STE) low byte; writing the others clears it, so it goes last
LINEWID         equ     $ffff820f       ; STE: words skipped after each line
COLOR0          equ     $ffff8240       ; the 16 colours, a word each
HSCROLL         equ     $ffff8265       ; STE: pixels the picture moves left, 0-15
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
WIDE            equ     320             ; bytes a line of the picture: 640 pixels, twice the screen's

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

        ; An STE? The cookie jar's _MCH cookie says: its top word is 1 for the STE and Mega STE.
        move.l  $5a0.w,d0               ; the cookie jar (0 before TOS 1.06: an ST)
        beq     not_ste
        move.l  d0,a0
.cookie:
        move.l  (a0)+,d0
        beq     not_ste                 ; the jar's end
        move.l  (a0)+,d1
        cmp.l   #$5f4d4348,d0           ; "_MCH"
        bne.s   .cookie
        swap    d1
        cmp.w   #1,d1
        bne     not_ste

        ; Memory for the picture: 200 lines of 320 bytes (the address no longer needs to be a multiple of 256
        ; on the STE, but this keeps it simple).
        move.l  #WIDE*200+255,-(sp)
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

not_ste:
        move.l  old_stack(pc),-(sp)
        move.w  #$20,-(sp)              ; Super: back to user mode
        trap    #1
        addq.l  #6,sp
        pea     needs_ste(pc)
        move.w  #9,-(sp)                ; Cconws: say why
        trap    #1
        addq.l  #6,sp
        move.w  #8,-(sp)                ; Cnecin: wait for a key
        trap    #1
        addq.l  #2,sp
        clr.w   -(sp)                   ; Pterm0
        trap    #1

; ---------------------------------------------------------------------------------------------------------------
; The VBL handler, at the start of each frame: Timer B is started counting lines afresh, the bars move on, and
; the picture is scrolled to where it is this frame, which the video takes up as the frame starts.

vbl:
        movem.l d0-d2/a0,-(sp)
        move.w  frames(pc),d0           ; x goes 0 to 320 and back with the frame count
        and.w   #511,d0
        cmp.w   #256,d0
        blo.s   .rising
        neg.w   d0
        add.w   #512,d0
.rising:
        move.w  d0,d1                   ; and a quarter more: 0 to 320
        lsr.w   #2,d1
        add.w   d1,d0
        move.w  d0,d1
        lsr.w   #4,d1                   ; the word it starts in,
        lsl.w   #3,d1                   ; 8 bytes a word of four planes
        ext.l   d1
        add.l   screen(pc),d1
        move.l  d1,d2
        swap    d2
        move.b  d2,DBASEH.w             ; the top byte, the middle, then the low
        move.w  d1,d2
        lsr.w   #8,d2
        move.b  d2,DBASEM.w
        move.b  d1,DBASEL.w
        and.w   #15,d0                  ; and the pixels within the word
        move.b  d0,HSCROLL.w
        beq.s   .whole
        move.b  #(WIDE-160)/2-4,LINEWID.w ; scrolled, the video reads a word more a plane each line
        bra.s   .scrolled
.whole: move.b  #(WIDE-160)/2,LINEWID.w
.scrolled:
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
        movem.l (sp)+,d0-d2/a0
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
; The picture: columns of colours 1 to 15, 16 pixels wide, in a band across the middle, 640 pixels wide. In low
; resolution each 16 pixels are four words, one a bitplane, and a pixel's colour is its bits in the four.

draw_screen:
        move.l  screen(pc),a0
        move.w  #WIDE*200/4-1,d0
.clear: clr.l   (a0)+
        dbra    d0,.clear
        move.l  screen(pc),a0
        lea     60*WIDE(a0),a0          ; from line 60
        move.w  #80-1,d1                ; for 80 lines
.line:  moveq   #0,d2                   ; the 40 columns across
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
        cmp.w   #40,d2
        bne.s   .column
        dbra    d1,.line
        rts

; ---------------------------------------------------------------------------------------------------------------

; STE colours: 16 levels a gun, each written with its lowest bit on top ($8 is level 1, $1 level 2, $F level 15).
palette:
        dc.w    $000,$f80,$f10,$f90,$f20,$fa0,$f30,$fb0
        dc.w    $ff0,$ef1,$df2,$cf3,$bf4,$af5,$9f6,$fff

; Colour 0 down the screen: blue to orange and back in 16 steps, twice round, so 32 starting places all have 26
; colours after them.
bar_colours:
        rept    2
        dc.w    $00f,$807,$18e,$986,$21d,$a15,$39c,$b94,$42b,$c23,$5aa,$da2,$639,$e31,$7b8,$fb0
        dc.w    $fb0,$7b8,$e31,$639,$da2,$5aa,$c23,$42b,$b94,$39c,$a15,$21d,$986,$18e,$807,$00f
        endr

needs_ste:
        dc.b    "This demo needs an STE or Mega STE.",13,10,"Press a key.",13,10,0

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
