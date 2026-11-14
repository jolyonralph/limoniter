' Colours at 64 columns
BORDER 0: PAPER 0: INK 7: CLS
PRINT "The eight colours, as PAPER and as INK:"
FOR c% = 0 TO 7
  PRINT AT 2 + c%, 0; PAPER c%; "        "; PAPER 0; INK c%; "  INK "; c%; INK 7; BRIGHT 1; "  bright";
  PRINT AT 2 + c%, 26; INK c%; BRIGHT 1; "INK "; c%
NEXT
PRINT AT 11, 0; "One colour cell holds two characters."
PRINT AT 12, 0; "Even column: ";
PRINT AT 12, 14; INK 2; "red"; INK 7; " - clean"
PRINT AT 13, 0; "Odd column:  ";
PRINT AT 13, 15; INK 2; "red"; INK 7; " - the space before it turns red too"
PRINT AT 15, 0; FLASH 1; INK 6; " FLASH "; FLASH 0; "  "; INVERSE 1; " INVERSE "; INVERSE 0
FOR x = 0 TO 255 STEP 32
  INK x / 32: PLOT x, 40: DRAW 31, 0: DRAW 0, -24: DRAW -31, 0: DRAW 0, 24
NEXT
INK 7
