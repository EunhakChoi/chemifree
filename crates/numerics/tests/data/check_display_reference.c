/* Independent cross-check of the authored formatting vectors in the C locale. */
#include <inttypes.h>
#include <fenv.h>
#include <float.h>
#include <locale.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc != 2 || FLT_RADIX != 2 || DBL_MANT_DIG != 53 || sizeof(double) != sizeof(uint64_t) ||
        setlocale(LC_NUMERIC, "C") == NULL || fesetround(FE_TONEAREST) != 0) return 2;
    FILE *file = fopen(argv[1], "r");
    if (!file) return 2;
    char row[1024], profile[16], input[17], places[8], expected[512], actual[512];
    unsigned checked = 0;
    while (fgets(row, sizeof row, file)) {
        if (row[0] == '#') continue;
        if (sscanf(row, "%15[^\t]\t%16[0-9a-f]\t%7[^\t]\t%511[^\n]", profile, input, places, expected) != 4) return 3;
        uint64_t raw = (uint64_t)strtoull(input, NULL, 16);
        double number;
        memcpy(&number, &raw, sizeof number);
        if (!isfinite(number)) return 3;
        if (!strcmp(profile, "default6")) {
            if (snprintf(actual, sizeof actual, "%.6g", number) < 0) return 3;
        } else if (!strcmp(profile, "fixed")) {
            int digits = atoi(places);
            if (digits < 0 || digits > 8 || snprintf(actual, sizeof actual, "%.*f", digits, number) < 0) return 3;
        } else if (!strcmp(profile, "native3")) {
            continue;
        } else return 3;
        if (strcmp(actual, expected)) {
            fprintf(stderr, "%s %s: C=%s expected=%s\n", profile, input, actual, expected);
            return 1;
        }
        ++checked;
    }
    if (ferror(file) || fclose(file) != 0) return 3;
    printf("C locale matched %u authored default and fixed formatting cases\n", checked);
    return checked == 2240 ? 0 : 1;
}
