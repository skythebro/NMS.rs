//! Patterns the game context and the lifecycle hook use. `nms-sdk` lists them in its `core` table so
//! `sigcheck` checks them with every mod's signatures. Each must match exactly once.

/// `cGcApplication::Update`, start of the function (`push rbx; sub rsp,0x20; call timer; mov [frame],rax;
/// call ...; cmp byte [..]`). Hooked by the core only; a mod hooking it is a `sigcheck` overlap error.
pub const UPDATE: &str = "40 53 48 83 EC 20 E8 ? ? ? ? 48 89 05 ? ? ? ? E8 ? ? ? ? 80 3D";
/// The two whole instructions the lifecycle jump replaces (`push rbx; sub rsp,0x20`): no
/// RIP-relative operand, so they run unchanged from the trampoline.
pub const UPDATE_PROLOGUE: [u8; 6] = [0x40, 0x53, 0x48, 0x83, 0xEC, 0x20];

/// The end of `cGcApplication::Update`: `lea rcx,[cGcApplication]`, `call cTkFSM::Update`, ..., tail jump.
pub const UPDATE_TAIL: &str = "F3 0F 10 0D ? ? ? ? 48 8D 0D ? ? ? ? F3 0F 59 0D ? ? ? ? E8 ? ? ? ? F0 FF 05 ? ? ? ? 48 8B 0D ? ? ? ? E8 ? ? ? ? 48 8D 0D ? ? ? ? E8 ? ? ? ? 48 83 C4 20 5B E9";
/// `UPDATE_TAIL` must sit at this offset from `UPDATE`.
pub const UPDATE_TAIL_AT: usize = 0x1E0;
/// rel32 of `lea rcx,[cGcApplication]` inside `UPDATE_TAIL`.
pub const TAIL_APP_DISP: usize = 11;
/// rel32 of `call cTkFSM::Update` inside `UPDATE_TAIL`.
pub const TAIL_FSM_CALL_DISP: usize = 0x18;

/// `cTkFSM::Update(fsm, dt)` (NMS.py pattern).
pub const FSM_UPDATE: &str = "48 89 5C 24 ? 57 48 83 EC ? 48 8B 05 ? ? ? ? 48 8B D9 0F 29 74 24";

/// `cTkFSM::Construct(fsm, table, initial)` (NMS.py pattern).
pub const FSM_CONSTRUCT: &str = "48 89 5C 24 ? 48 89 6C 24 ? 48 89 74 24 ? 57 48 83 EC ? 33 ED 48 89 51";

/// The one call of `cTkFSM::Construct`, in the `cGcApplication` constructor:
/// `lea r8,[initial id]; lea rdx,[state table]; lea rcx,[cGcApplication]; call Construct`.
pub const FSM_CONSTRUCT_CALL: &str = "4C 8D 05 ? ? ? ? 48 8D 15 ? ? ? ? 48 8D 0D ? ? ? ? E8 ? ? ? ? 48 8B 05 ? ? ? ? 33 D2 66 89 3D";
pub const CONSTRUCT_TABLE_DISP: usize = 10;
pub const CONSTRUCT_APP_DISP: usize = 17;
pub const CONSTRUCT_CALL_DISP: usize = 22;

/// The first three getters after `cGcApplication::Update` (simulation, reality, game state). The
/// third reads the data pointer straight from its global, which gives the data slot.
pub const APP_GETTERS: &str = "48 8B 41 38 48 05 ? ? ? ? C3 CC CC CC CC CC 48 8B 41 38 48 83 C0 60 C3 CC CC CC CC CC CC CC 48 8B 05 ? ? ? ? 48 05 ? ? ? ? C3";
/// rel32 of `mov rax,[rip+data slot]` inside `APP_GETTERS`.
pub const GETTERS_SLOT_DISP: usize = 35;
