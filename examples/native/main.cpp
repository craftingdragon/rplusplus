extern "C" int rust_value();
extern "C" int extra_value();
int main() { return rust_value() + extra_value() == 42 ? 0 : 1; }
