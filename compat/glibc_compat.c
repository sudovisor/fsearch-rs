/* Bind libm functions to their oldest x86_64 symbol versions, so the binary (and the AppImage)
 * runs on glibc 2.34+ instead of the build host's glibc. The old versions are the same functions
 * with less strict error-reporting (errno) behavior, which nothing here relies on. */
__asm__(".symver atan2f_compat, atan2f@GLIBC_2.2.5");
__asm__(".symver hypot_compat, hypot@GLIBC_2.2.5");
__asm__(".symver hypotf_compat, hypotf@GLIBC_2.2.5");
__asm__(".symver fmod_compat, fmod@GLIBC_2.2.5");
float atan2f_compat(float, float);
double hypot_compat(double, double);
float hypotf_compat(float, float);
double fmod_compat(double, double);
float __wrap_atan2f(float y, float x) { return atan2f_compat(y, x); }
double __wrap_hypot(double x, double y) { return hypot_compat(x, y); }
float __wrap_hypotf(float x, float y) { return hypotf_compat(x, y); }
double __wrap_fmod(double x, double y) { return fmod_compat(x, y); }
