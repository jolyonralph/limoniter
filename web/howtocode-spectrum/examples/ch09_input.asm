; ch09_input.asm: the keyboard's eight half-rows and a Kempston joystick, read each frame and shown: a key held
; down lights up where it's printed. Each half-row is read with IN from port $FE, its high byte choosing the row
; (a 0 bit for each row wanted); a key pressed reads as a 0 in bits 0 to 4. The Kempston is port $1F, its bits 1
; when pressed. SPACE, as always, goes back to BASIC. Chapter 9, Input. CC0.

AUTOEXIT        equ     0               ; frames before going back to BASIC: 0 for never (SPACE goes back too)

        device  zxspectrum48
        include "startup.asm"

PLAIN           equ     %00111000       ; black on white
LIT             equ     %01010111       ; bright white on red

Init:
        ld      a,PLAIN
        call    ClearScreen
        ld      hl,Lines                ; each line: its row, then its text
.line:
        ld      b,(hl)
        inc     b
        ret     z                       ; a row of $FF ends them
        dec     b
        inc     hl
        ld      c,0
        call    PrintAt
        inc     hl
        jr      .line

Frame:
        ; The keyboard: each half-row read, and its five keys' cells lit or not.
        ld      hl,$5800+2*32+8         ; the first key's cell: row 2, column 8
        ld      de,HalfRows
        ld      b,8
.half_row:
        push    bc
        ld      a,(de)                  ; the port's high byte: $FE, $FD, $FB ... $7F
        inc     de
        in      a,($FE)                 ; IN A,(n) puts A on the high byte of the port
        ld      c,a
        push    hl
        ld      b,5
.key:
        rr      c                       ; bit 0 first: 0 if it's pressed
        ld      a,LIT
        jr      nc,.show
        ld      a,PLAIN
.show:
        call    Light
        djnz    .key
        pop     hl
        ld      bc,32
        add     hl,bc
        pop     bc
        djnz    .half_row
        ; The Kempston: right, left, down, up and fire in bits 0 to 4, 1 when pressed. With none plugged in, the
        ; port isn't answered, and reads $FF here in the top border: everything pressed at once, which no joystick
        ; can be (left and right together), so it's taken as no joystick.
        in      a,($1F)
        ld      c,a
        and     %00000011
        cp      %00000011
        ld      hl,$5800+12*32+8
        jr      nz,.kempston
        ld      c,0                     ; nothing lit
.kempston:
        ld      b,5
.direction:
        rr      c
        ld      a,PLAIN
        jr      nc,.lit
        ld      a,LIT
.lit:
        call    Light
        djnz    .direction
        ret

; Light: the five cells from HL made attribute A, and HL moved on to the next five.
Light:
        ld      (hl),a
        inc     hl
        ld      (hl),a
        inc     hl
        ld      (hl),a
        inc     hl
        ld      (hl),a
        inc     hl
        ld      (hl),a
        inc     hl
        ret

Interrupt:
        ret

; The half-rows in the order of the lines below: the high byte that reads each.
HalfRows:       db      $FE,$FD,$FB,$F7,$EF,$DF,$BF,$7F
Lines:          db      0,"Port    bit 0 to bit 4",0
                db      2,"$FEFE   CAPS Z    X    C    V",0
                db      3,"$FDFE   A    S    D    F    G",0
                db      4,"$FBFE   Q    W    E    R    T",0
                db      5,"$F7FE   1    2    3    4    5",0
                db      6,"$EFFE   0    9    8    7    6",0
                db      7,"$DFFE   P    O    I    U    Y",0
                db      8,"$BFFE   ENTERL    K    J    H",0
                db      9,"$7FFE   SPACESYM  M    N    B",0
                db      11,"Kempston joystick",0
                db      12,"$1F     rightleft down up   fire",0
                db      15,"Sinclair 1: 6 7 8 9 0, left,",0
                db      16,"  right, down, up, fire",0
                db      17,"Cursor: 5 8 6 7 0, the same",0
                db      $FF
