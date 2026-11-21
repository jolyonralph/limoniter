; ch12_game.asm: Catch, a small whole game on the startup. Stars fall; move the paddle along the bottom with O and
; P (or a Kempston joystick, or Sinclair joystick 1: keys 6 and 7) to catch them. Each one caught scores a point
; and clicks the beeper; each one missed costs one of three lives, and the stars fall faster as the score goes up.
; When the lives are gone it's game over until ENTER (or fire) starts another. Everything is drawn with
; attributes alone, one character cell a block, so the drawing is a few bytes a frame. It has two states,
; playing and over, a random number generator (xorshift), and reads every control the same way.
; Chapter 12, Games and demos. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

SKY             equ     %00001000       ; blue paper
STAR            equ     %01110000       ; bright yellow paper
PADDLE          equ     %01101000       ; bright cyan paper
GROUND          equ     %00100000       ; green paper
TOP_ROW         equ     2               ; stars fall from row 2 to row 20; the paddle is on row 21
PADDLE_ROW      equ     21
STARS           equ     4
LEFT            equ     1               ; what ReadControls returns: bits for left, right and fire
RIGHT           equ     2
FIRE            equ     4

Init:
        ld      a,SKY
        call    ClearScreen
        ld      hl,$5800+22*32          ; the ground: rows 22 and 23
        ld      b,64
.ground:
        ld      (hl),GROUND
        inc     hl
        djnz    .ground
        ld      hl,$5800                ; the top line black on white, for the score
        ld      b,32
.top:
        ld      (hl),%00111000
        inc     hl
        djnz    .top
        ld      bc,$0000
        ld      hl,ScoreText
        call    PrintAt
        ld      a,(Frames)              ; a seed for the random numbers: never 0
        or      1
        ld      (Seed),a
        jp      NewGame

Frame:
        ld      a,(State)
        or      a
        jr      nz,Over
Playing:
        call    MovePaddle
        call    MoveStars
        jp      ShowScore

; Game over: the stars stop; ENTER or fire starts again.
Over:
        call    ReadControls
        and     FIRE
        ret     z
NewGame:
        xor     a
        ld      (State),a
        ld      (Score),a
        ld      a,3
        ld      (Lives),a
        ld      a,15
        ld      (PaddleX),a
        ld      a,12                    ; frames for a star to fall a row, fewer as the score goes up
        ld      (Speed),a
        ld      hl,$5800+TOP_ROW*32     ; the sky cleared of stars, and the message
        ld      bc,20*32-1
        ld      (hl),SKY
        ld      d,h
        ld      e,l
        inc     de
        ldir
        ld      hl,Stars                ; every star waiting to start, a different time each
        ld      b,STARS
        ld      c,1
.star:
        ld      (hl),0                  ; row 0: not falling
        inc     hl
        ld      (hl),0                  ; column
        inc     hl
        ld      (hl),c                  ; frames until it moves (or starts)
        inc     hl
        ld      a,c
        add     a,20
        ld      c,a
        djnz    .star
        call    DrawPaddle
        jp      ShowScore

; Each star: its row (0 while it waits to start), its column, and frames until it moves.
MoveStars:
        ld      ix,Stars
        ld      b,STARS
.star:
        push    bc
        dec     (ix+2)
        jp      nz,.next
        ld      a,(Speed)
        ld      (ix+2),a
        ld      a,(ix+0)
        or      a
        jr      nz,.falling
        call    Random                  ; start it: a column from 1 to 30
        and     31
        cp      31
        jr      c,.column
        sub     16
.column:
        or      a
        jr      nz,.start
        inc     a
.start:
        ld      (ix+1),a
        ld      (ix+0),TOP_ROW
        ld      c,STAR
        call    Cell
        jr      .next
.falling:
        ld      c,SKY                   ; rubbed out where it was
        call    Cell
        inc     (ix+0)
        ld      a,(ix+0)
        cp      PADDLE_ROW
        jr      z,.landed
        ld      c,STAR
        call    Cell
        jr      .next
.landed:
        ld      (ix+0),0                ; it waits to start again
        ld      a,(PaddleX)             ; caught if it's over one of the paddle's three cells
        ld      c,a
        ld      a,(ix+1)
        sub     c
        cp      3
        jr      nc,.missed
        ld      hl,Score
        inc     (hl)
        ld      a,(hl)
        and     3                       ; faster every four points, down to a row every 3 frames
        jr      nz,.click
        ld      a,(Speed)
        cp      4
        jr      c,.click
        dec     a
        ld      (Speed),a
.click:
        call    Click
        jr      .next
.missed:
        ld      hl,Lives
        dec     (hl)
        jr      nz,.next
        ld      a,1                     ; no lives left: game over
        ld      (State),a
        ld      bc,$0C0B
        ld      hl,OverText
        call    PrintAt
        ld      bc,$0D06
        ld      hl,AgainText
        call    PrintAt
.next:
        ld      de,3
        add     ix,de
        pop     bc
        dec     b
        jp      nz,.star
        ret

; Cell: the cell at row (IX+0), column (IX+1) given attribute C.
Cell:
        ld      a,(ix+0)
        ld      l,a
        ld      h,0
        add     hl,hl
        add     hl,hl
        add     hl,hl
        add     hl,hl
        add     hl,hl                   ; row x 32
        ld      a,(ix+1)
        add     a,l
        ld      l,a
        ld      a,h
        adc     a,$58
        ld      h,a
        ld      (hl),c
        ret

MovePaddle:
        call    ReadControls
        ld      b,a
        ld      a,(PaddleX)
        bit     0,b                     ; LEFT
        jr      z,.not_left
        or      a
        jr      z,.not_left
        dec     a
.not_left:
        bit     1,b                     ; RIGHT
        jr      z,.not_right
        cp      29
        jr      nc,.not_right
        inc     a
.not_right:
        ld      (PaddleX),a
DrawPaddle:
        ld      hl,$5800+PADDLE_ROW*32  ; the row cleared, then three cells of paddle
        ld      b,32
.clear:
        ld      (hl),SKY
        inc     hl
        djnz    .clear
        ld      a,(PaddleX)
        ld      l,a
        ld      h,0
        ld      de,$5800+PADDLE_ROW*32
        add     hl,de
        ld      (hl),PADDLE
        inc     hl
        ld      (hl),PADDLE
        inc     hl
        ld      (hl),PADDLE
        ret

; ReadControls: A's bits 0 to 2 set for left, right and fire, from the keys (O, P, ENTER), Sinclair joystick 1
; (6, 7, 0) and a Kempston joystick, whichever is used.
ReadControls:
        ld      c,0
        ld      a,$DF                   ; P O I U Y: P is bit 0, O bit 1
        in      a,($FE)
        bit     1,a
        jr      nz,.o
        set     0,c
.o:
        bit     0,a
        jr      nz,.p
        set     1,c
.p:
        ld      a,$BF                   ; ENTER L K J H: ENTER is bit 0
        in      a,($FE)
        bit     0,a
        jr      nz,.enter
        set     2,c
.enter:
        ld      a,$EF                   ; 0 9 8 7 6: 6 left (bit 4), 7 right (bit 3), 0 fire (bit 0)
        in      a,($FE)
        bit     4,a
        jr      nz,.six
        set     0,c
.six:
        bit     3,a
        jr      nz,.seven
        set     1,c
.seven:
        bit     0,a
        jr      nz,.zero
        set     2,c
.zero:
        in      a,($1F)                 ; Kempston: right bit 0, left bit 1, fire bit 4, 1 when pressed
        ld      b,a
        and     %00000011
        cp      %00000011               ; left and right at once: no joystick there
        jr      z,.done
        bit     0,b
        jr      z,.k_right
        set     1,c
.k_right:
        bit     1,b
        jr      z,.k_left
        set     0,c
.k_left:
        bit     4,b
        jr      z,.done
        set     2,c
.done:
        ld      a,c
        ret

; Random: the next of 255 numbers from 1 to 255 (an 8-bit xorshift: x ^= x << 1, x ^= x >> 1, x ^= x << 2).
Random:
        ld      a,(Seed)
        ld      b,a
        add     a,a
        xor     b
        ld      b,a
        srl     a
        xor     b
        ld      b,a
        add     a,a
        add     a,a
        xor     b
        ld      (Seed),a
        ret

; Click: a short tick from the beeper, 32 changes of bit 4 of port $FE.
Click:
        ld      b,32
        ld      a,%00010000
.tick:
        out     ($FE),a
        xor     %00010000
        ld      c,40
.wait:
        dec     c
        jr      nz,.wait
        djnz    .tick
        ret

ShowScore:
        ld      a,(Score)
        ld      l,a
        ld      h,0
        ld      bc,$0007
        call    PrintNumber
        ld      a,(Lives)
        add     a,'0'
        ld      bc,$001F
        jp      PrintChar

Interrupt:
        ret

State:          db      0               ; 0 playing, 1 over
Score:          db      0
Lives:          db      0
PaddleX:        db      0
Speed:          db      0
Seed:           db      1
Stars:          ds      STARS*3
ScoreText:      db      "Score         Catch!     Lives",0
OverText:       db      "GAME OVER",0
AgainText:      db      "ENTER or fire to play",0
