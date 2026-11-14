' The Union Flag, drawn by colour cells and lines
' The flag fills the top 16 character rows (y 64-191); the rows below stay black.
' Colours are laid out by 8x8 colour cell: blue PAPER for the field, white INK
' lines for the diagonals, red PAPER showing through cleared pixels for the
' thin red diagonals, and whole cells of PAPER for the central cross.
DEF PROC span(x1, x2, y)
  ' one horizontal run of pixels, clipped to the flag's width
  IF x1 < 0 THEN x1 = 0
  IF x2 > 255 THEN x2 = 255
  IF x1 <= x2 THEN PLOT x1, y: DRAW x2 - x1, 0
ENDPROC
DEF PROC field
  ' the blue field: 16 rows of blue paper
  PAPER 1: INK 7: BRIGHT 0
  FOR r = 0 TO 15
    PRINT AT r, 0; STRING$(64, " ");
  NEXT r
ENDPROC
DEF PROC diagonals
  ' the white saltire: two bands, 65 pixels across, corner to corner
  INK 7: PAPER 1: INVERSE 0
  FOR r = 0 TO 127
    a = 0
    IF r < 64 THEN a = 1
    span 2 * r - 32, 2 * r + 32 + a, 191 - r
    span 223 + a - 2 * r, 287 - 2 * r, 191 - r
  NEXT r
ENDPROC
DEF PROC edges(top, bottom)
  ' white pixel rows of the cross's border that share cells with the diagonals
  INK 7: PAPER 1: INVERSE 0
  FOR r = top TO bottom
    span 0, 255, 191 - r
  NEXT r
ENDPROC
DEF PROC red_diagonals
  ' the thin red diagonals, offset from the centre of each white band;
  ' clearing pixels lets the red paper show while the white ink stays
  INK 7: PAPER 2: INVERSE 1
  FOR r = 0 TO 35
    span 2 * r - 15, 2 * r + 2, 191 - r
    span 239 - 2 * r, 254 - 2 * r, 191 - r
  NEXT r
  FOR r = 92 TO 127
    span 256 - 2 * r, 271 - 2 * r, 191 - r
    span 2 * r - 1, 2 * r + 15, 191 - r
  NEXT r
  INVERSE 0
ENDPROC
DEF PROC cross
  ' the central cross in whole cells: white border, red centre
  PRINT AT 5, 0; PAPER 7; STRING$(64, " ");
  PRINT AT 10, 0; PAPER 7; STRING$(64, " ");
  FOR r = 6 TO 9
    PRINT AT r, 0; PAPER 2; STRING$(64, " ");
  NEXT r
  FOR r = 0 TO 15
    IF r < 5 OR r > 10 THEN PRINT AT r, 26; PAPER 7; "  "; PAPER 2; "        "; PAPER 7; "  ";
    IF r >= 5 AND r <= 10 THEN PRINT AT r, 28; PAPER 2; "        ";
  NEXT r
ENDPROC
' main program
t = FRAMES
BORDER 0: PAPER 0: INK 7: CLS
field
diagonals
edges 36, 39
edges 88, 91
red_diagonals
cross
PAPER 0: INK 7
PRINT AT 22, 0; "Drawn in "; INT((FRAMES - t) / 5) / 10; " seconds";
