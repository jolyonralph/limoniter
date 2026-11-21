; machine:vectrex
; A Vectrex cartridge that needs no BIOS: it sets the VIA up itself, puts a tone on the AY, and each frame reads the
; buttons and the first joystick, zeroes the beam and draws a square, waiting for timer 2 between frames.
VIA_port_b      equ $D000
VIA_port_a      equ $D001
VIA_DDR_b       equ $D002
VIA_DDR_a       equ $D003
VIA_t1_cnt_lo   equ $D004
VIA_t1_cnt_hi   equ $D005
VIA_t2_lo       equ $D008
VIA_t2_hi       equ $D009
VIA_shift_reg   equ $D00A
VIA_aux_cntl    equ $D00B
VIA_cntl        equ $D00C
VIA_int_flags   equ $D00D
buttons         equ $C880
stick           equ $C881
count           equ $C882

        org 0
        fcc "g GCE 2026"
        fcb $80
        fdb $FD0D
        fcb $F8,$50,$20,-$40
        fcc "SQUARE"
        fcb $80
        fcb 0

start:
        lds #$CBEA
        lda #$9F
        sta VIA_DDR_b           ; PB0-PB4 and PB7 out; PB5 (the comparator) and PB6 in
        lda #$FF
        sta VIA_DDR_a
        lda #$98
        sta VIA_aux_cntl        ; timer 1 drives RAMP, the shift register BLANK
        lda #$81
        sta VIA_port_b          ; the multiplexer off
        clr VIA_shift_reg       ; the beam off
        ldx #tune
tone:   lda ,x+
        bmi frame
        ldb ,x+
        lbsr ay_write
        bra tone

frame:
        lda #$30
        sta VIA_t2_lo
        lda #$75
        sta VIA_t2_hi           ; 30,000 cycles: a fiftieth of a second
        lbsr read_controls
        lda #$CC
        sta VIA_cntl            ; ZERO held: the beam to the middle
        clr VIA_port_a
        lda #$82
        sta VIA_port_b          ; the zero reference to 0
        lda #$81
        sta VIA_port_b
        lda #$7F
        sta VIA_port_a
        lda #$84
        sta VIA_port_b          ; the brightness to $7F
        lda #$81
        sta VIA_port_b
        lda #$CE
        sta VIA_cntl            ; ZERO let go
        ldd #$E0E0              ; to the corner, (-32, -32), dark
        clr count
        bsr move
        ldx #square
        lda #4
        sta count
side:   ldd ,x++
        bsr move
        dec count
        bne side
wait:   lda VIA_int_flags
        bita #$20
        beq wait
        bra frame

; Moves the beam by A (up) and B (across) for 127 cycles; lit if count isn't 0.
move:   sta VIA_port_a          ; Y to the DAC
        clr VIA_port_b          ; the multiplexer on to Y
        lda #$01
        sta VIA_port_b
        stb VIA_port_a          ; X to the DAC
        lda #$7F
        sta VIA_t1_cnt_lo
        lda count
        beq dark
        lda #$FF
        sta VIA_shift_reg       ; the beam on
dark:   clr VIA_t1_cnt_hi       ; RAMP until timer 1 runs out
mwait:  lda VIA_int_flags
        bita #$40
        beq mwait
        clr VIA_shift_reg       ; the beam off
        rts

; Register A of the AY gets B.
ay_write:
        sta VIA_port_a
        lda #$19
        sta VIA_port_b          ; BDIR and BC1: latch the register's number
        lda #$01
        sta VIA_port_b
        stb VIA_port_a
        lda #$11
        sta VIA_port_b          ; BDIR: write
        lda #$01
        sta VIA_port_b
        rts

; The buttons (the AY's register 14), and whether the first joystick is pushed right.
read_controls:
        lda #14
        sta VIA_port_a
        lda #$19
        sta VIA_port_b
        lda #$01
        sta VIA_port_b
        clr VIA_DDR_a           ; port A turned round, to read
        lda #$09
        sta VIA_port_b          ; BC1: read
        lda VIA_port_a
        sta buttons
        lda #$01
        sta VIA_port_b
        lda #$FF
        sta VIA_DDR_a
        clr VIA_port_a          ; the DAC at 0, the comparator on the first joystick across
        clr VIA_port_b
        lda VIA_port_b
        anda #$20
        sta stick
        lda #$01
        sta VIA_port_b
        rts

tune:   fcb 0,$00, 1,$01, 7,$3E, 8,$0C, $FF
square: fcb $40,$00, $00,$40, -$40,$00, $00,-$40
