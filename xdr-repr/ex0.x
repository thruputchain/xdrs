        // Fun stuff!
        const FOO = -1;
        const BAR = 234229;

        enum Result {
            OK = 0,
            ERROR = 1,
        };

        struct NotMuch {
            void;
            void
        };

        struct Foo {
            int bar;
            int baz[3];
            int quux<4>;
            opaque buf[3];
        };

        typedef int Number;

        union Blargh switch (int okay) {
            case 1:
                struct {
                    unsigned int red;
                    unsigned int green;
                    unsigned int blue;
                } color;
            case 0:
            case -1:
                opaque malady<>;
            default:
                void;
        };
