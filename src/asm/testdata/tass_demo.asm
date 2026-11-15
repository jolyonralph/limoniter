; A raster bar and a clear-screen routine, in 64tass's way, to check Limoniter's
; assembler makes what 64tass -a makes.
BORDER  = $d020
BACK    = $d021
screen  = $0400
speed   := 2

        *= $0801
        .word (+), 2026
        .null $9e, "2061"
+       .word 0

start   sei
        lda #$7f
        sta $dc0d
        sta $dd0d
        lda #$35
        sta $01
        lda #<irq
        sta $fffe
        lda #>irq
        sta $ffff
        lda #$1b
        sta $d011
        lda #$80
        sta $d012
        lda #$01
        sta $d01a
        lda $dc0d
        asl $d019
        cli
        jsr clear
-       jmp -

clear   .proc
        ldx #0
        lda #' '
loop    sta screen,x
        sta screen+$100,x
        sta screen+$200,x
        sta screen+$300,x
        inx
        bne loop
        rts
        .pend

never   .proc
        brk
        .pend

irq     .block
        pha
        txa
        pha
        ldx #0
-       lda colours,x
        sta BORDER
        sta BACK
        ldy #8
-       dey
        bne -
        inx
        cpx #colours_end-colours
        bne --
        lda #0
        sta BORDER
        sta BACK
        asl $d019
        pla
        tax
        pla
        rti
        .bend

colours
        .for i = 0, i < 8, i += 1
        .byte i
        .next
        .byte 2, 8, 7, 1
colours_end
message .text "hello, 64tass"
        .byte 0
        .null "Mixed Case"
        .ptext "abc"
        .shift "abc"
        .word start, irq, clear.loop
        .rta start
        .long $123456
        .dword $12345678
        .fill 4, $ea
        .align $10
table   .byte <table, >table
        .logical $c000
moved   jmp moved
        .here
        .if speed == 2
        nop
        .else
        brk
        .fi
        .rept 3
        nop
        .next
        beq +
        bne ++
+       nop
+       rts
        .enc "screen"
        .text "screen"
