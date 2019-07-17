#include <stdint.h>

struct OneInt {
  int el1;
};

struct TwoInts {
  int el1;
  int el2;
};

struct ThreeInts {
  int el1;
  int el2;
  int el3;
};

struct Mismatched {
  uint8_t el1;
  uint32_t el2;
  uint8_t el3;
};

struct Nested {
  struct TwoInts ti;
  struct Mismatched mm;
};

struct WithArray {
  struct Mismatched mm;
  int arr[10];
  struct Mismatched mm2;
};

// ensure we can read and write from OneInt
int one_int(int x) {
  volatile struct OneInt oi = { 0 };
  oi.el1 = x;
  return oi.el1 - 3;
}

// ensure we can read and write from first field in TwoInts
int two_ints_first(int x) {
  volatile struct TwoInts ti = { 0 };
  ti.el1 = x;
  return ti.el1 - 3;
}

// ensure we can read and write from second field in TwoInts
int two_ints_second(int x) {
  volatile struct TwoInts ti = { 0 };
  ti.el2 = x;
  return ti.el2 - 3;
}

// ensure we can read and write from both TwoInts fields without getting them confused
int two_ints_both(int x) {
  volatile struct TwoInts ti = { 0 };
  ti.el1 = x + 2;
  ti.el2 = x + 3;
  ti.el1 = ti.el2 - 10;
  ti.el2 = ti.el1 + 7;
  return ti.el2 - 3;
}

// ensure we can read and write from all fields in ThreeInts
int three_ints(int x, int y) {
  volatile struct ThreeInts ti = { 0 };
  ti.el1 = x + y;
  ti.el2 = x - y;
  ti.el3 = ti.el1 + ti.el2;
  ti.el2 = ti.el3 - 2 * ti.el1;
  ti.el1 = ti.el3 - x;
  return ti.el1 - 3;
}

// ensure we can read and write from all fields in Mismatched
int mismatched(uint8_t x, int y) {
  volatile struct Mismatched mm = { 0 };
  mm.el1 = x + 3;
  mm.el2 = y - 3;
  mm.el3 = mm.el1 - x;
  mm.el1 = mm.el2 - mm.el3;
  mm.el2 = mm.el3 + 4;
  mm.el1 = mm.el2 - x;
  mm.el3 = mm.el2 - 5;
  mm.el2 = mm.el1 + y;
  return mm.el2 + 3*x;
}

// ensure we can read and write from all fields in Nested
int nested(uint8_t x, int y) {
  volatile struct Nested n = { 0 };
  n.ti.el2 = y + 3;
  n.mm.el1 = x - 4;
  n.ti.el1 = n.mm.el2 + y;
  n.mm.el3 = n.mm.el1 + 10;
  n.mm.el2 = n.mm.el3 + n.mm.el1;
  n.ti.el2 = n.mm.el3 + n.ti.el1;
  return n.ti.el2 - y;
}

// ensure we can read and write from all fields in WithArray
int with_array(int x) {
  volatile struct WithArray wa = { 0 };
  wa.arr[2] = x + 4;
  wa.arr[4] = wa.arr[5] - 3;
  wa.mm.el2 = wa.arr[2];
  wa.mm2.el2 = wa.arr[2] + x;
  return wa.arr[4] - wa.mm2.el2;
}

// manipulate a struct through a pointer
int structptr(int x) {
  volatile struct Mismatched _mm = { 0 };
  volatile struct Mismatched* mm = &_mm;
  mm->el2 = x + 4;
  mm->el1 = mm->el3 + x;
  return mm->el2 + mm->el1;
}

// tons of pointer shenanigans
int ptrs(int x) {
  volatile struct WithArray wa1 = { 0 };
  volatile struct WithArray wa2 = { 0 };
  volatile struct WithArray* waptr = &wa1;
  waptr->arr[3] = x + 4;
  waptr = &wa2;
  waptr->arr[4] = x + 7;
  waptr->mm2.el2 = wa1.mm.el2 + 3;
  volatile int* arrptr = &wa1.arr[0];
  arrptr[7] = waptr->arr[4] + wa1.arr[3];
  volatile int* arrptr2 = &waptr->arr[0];
  arrptr2[1] = waptr->arr[7] - wa2.mm2.el2;
  arrptr2 = arrptr;
  arrptr2[5] = wa1.mm.el2 + wa1.arr[3];
  wa2.mm.el2 = waptr->mm2.el2 + 3;
  return wa2.mm.el2 + waptr->arr[1] + arrptr2[5] + wa1.arr[5];
}
