' The computer guesses your number by halving
secret% = 37: lo% = 1: hi% = 100: tries% = 0
REPEAT
  g% = (lo% + hi%) DIV 2
  tries% = tries% + 1
  CASE SGN(g% - secret%) OF
    WHEN -1
      PRINT g%; " is too low"
      lo% = g% + 1
    WHEN 1
      PRINT g%; " is too high"
      hi% = g% - 1
    OTHERWISE
      PRINT "Got it: "; g%; " in "; tries%; " tries"
  ENDCASE
UNTIL g% = secret%
