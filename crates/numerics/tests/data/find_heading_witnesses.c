#include <fenv.h>
#include <float.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint64_t state = UINT64_C(0x2489ab42c511e787);

static uint64_t next(void) {
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    return state;
}

static double sample(void) {
    uint64_t bits = next();
    bits = (bits & UINT64_C(0x800fffffffffffff)) | ((UINT64_C(0x3fc) + (next() % 13)) << 52);
    double out;
    memcpy(&out, &bits, sizeof out);
    return out;
}

static uint64_t bits(double value) {
    uint64_t out;
    memcpy(&out, &value, sizeof out);
    return out;
}

static long double delta(double turn) {
    volatile long double product = (long double)turn * (long double)0x1.921fb54442d18p+1;
    volatile long double result = product / (long double)180.0;
    return result;
}

int main(void) {
    if (FLT_RADIX != 2 || DBL_MANT_DIG != 53 || LDBL_MANT_DIG != 64 || LDBL_MAX_EXP != 16384 || fesetround(FE_TONEAREST) != 0) return 2;
    unsigned found_double = 0, found_reassoc = 0, found_store = 0;
    for (unsigned i = 0; i < 500000 && (found_double < 3 || found_reassoc < 3 || found_store < 3); ++i) {
        double h = sample(), t = sample(), t2 = sample();
        volatile long double d = delta(t);
        volatile long double sum = (long double)h + d;
        volatile double result = (double)sum;
        volatile double dp = t * 0x1.921fb54442d18p+1;
        volatile double dd = dp / 180.0;
        volatile double dr = h + dd;
        volatile long double factor = (long double)0x1.921fb54442d18p+1 / (long double)180.0;
        volatile long double ra = (long double)t * factor;
        volatile double rr = (double)((long double)h + ra);
        volatile long double next_delta = delta(t2);
        volatile double stored = (double)((long double)result + next_delta);
        volatile double unstored = (double)(sum + next_delta);
        const char *category = NULL;
        if (found_double < 3 && bits(result) != bits(dr)) { ++found_double; category = "binary64"; }
        else if (found_reassoc < 3 && bits(result) != bits(rr)) { ++found_reassoc; category = "reassociate"; }
        else if (found_store < 3 && bits(stored) != bits(unstored)) { ++found_store; category = "prior-store"; }
        if (category) printf("%s\th=%016" PRIx64 "\tt=%016" PRIx64 "\tt2=%016" PRIx64 "\tfirst=%016" PRIx64 "\tsecond=%016" PRIx64 "\n", category, bits(h), bits(t), bits(t2), bits(result), bits(stored));
    }
    return found_double < 3 || found_reassoc < 3 || found_store < 3;
}
