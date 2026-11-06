// Types only, generated with every derive feature suppressed: a union's
// default arm must not leave an attribute behind that no derive declares.
// See check.sh.

enum Level {
  LOW = 0,
  HIGH = 1
};

union Reading switch (Level l) {
case LOW:
  int raw;
default:
  void;
};

union Count switch (int n) {
case 1:
  int one;
default:
  int other;
};

struct Sample {
  Reading reading;
  Count count;
};
