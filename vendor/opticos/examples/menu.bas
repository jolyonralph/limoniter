' OpticOS example tape: the menu
DEF PROC menu
  BORDER 0: PAPER 0: INK 7: CLS
  INK 6: PRINT "OpticOS example tape": INK 7
  PRINT
  RESTORE menu
  REPEAT
    READ n$, d$
    IF n$ <> "" THEN PRINT TAB(2); "LOAD """; n$; """"; TAB(22); d$
  UNTIL n$ = ""
  PRINT
  PRINT "Type a LOAD line, then RUN. LIST shows the program."
  DATA "times", "a times table with TAB and INK"
  DATA "guess", "the computer halves its way to a number"
  DATA "fact", "recursive functions"
  DATA "sort", "DATA, RESTORE name, SWAP and arrays"
  DATA "errors", "TRY, CATCH and ERROR n, msg$"
  DATA "ball", "a bouncing ball; SPACE stops it"
  DATA "tree", "a recursive tree drawn with DRAW TO"
  DATA "colour", "the eight colours and the clash rule"
  DATA "hilo", "guess my number (INPUT, TRY, CASE)"
  DATA "flag", "the Union Flag in colour cells and DRAW"
  DATA "", ""
ENDPROC
