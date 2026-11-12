#ifndef EXPECTED
#define EXPECTED 1
#endif
extern "C" int answer();
extern "C" int build_mode();
int main() { return answer() == 42 && build_mode() == EXPECTED ? 0 : 1; }
