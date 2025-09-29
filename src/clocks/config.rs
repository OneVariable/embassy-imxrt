use crate::peripherals::{PIO0_25, PIO2_15, PIO2_30};

#[derive(Clone, Copy, Debug)]
pub struct ClockConfig {
    /// Clock coming from RTC crystal oscillator
    pub enable_32k_clk: bool,
    pub enable_16m_irc: bool,
    pub enable_1m_lposc: bool,
    pub _48_60m_irc_select: _48_60mIrcSelect,
    pub _32k_wake_clk_select: _32kWakeClkSelect,
    pub main_pll: Option<MainPll>,
    pub main_clock_select: MainClockSelect,
    /// Main clock divided by (1 + sys_cpu_ahb_div)
    pub sys_cpu_ahb_div: u8,
    /// Division of FRGPLLCLKDIV, main_pll_clk divided by (1 + frg_clk_pll_div)
    /// TODO: Switch to `u16` with check? At least: MAKE IT CONSISTENT.
    /// TODO: Check what stm32 HALs do for this kind of thing?
    pub frg_clk_pll_div: Option<u8>,
    pub clk_out_select: ClockOutSource,
    pub clk_out_div: Option<u8>,
}

impl Default for ClockConfig {
    fn default() -> Self {
        todo!()
    }
}

/// ```text
///      16m_irc ┌─────┐                          ┌─────┐
/// ────────────▶│000  │      ┌──────────────────▶│000  │
///       clk_in │     │      │      main_pll_clk │     │
/// ────────────▶│001  │      │      ────────────▶│001  │
///     1m_lposc │     │      │      aux0_pll_clk │     │
/// ────────────▶│010  │      │      ────────────▶│010  │
///   48/60m_irc │     │──────┘       dsp_pll_clk │     │    ┌───────┐
/// ────────────▶│011  │             ────────────▶│011  │    │CLKOUT │    CLKOUT
///     main_clk │     │             aux1_pll_clk │     │───▶│Divider│────────────▶
/// ────────────▶│100  │             ────────────▶│100  │    └───────┘
/// dsp_main_clk │     │            audio_pll_clk │     │        ▲
/// ────────────▶│110  │             ────────────▶│101  │        │
///              └─────┘                  32k_clk │     │    CLKOUTDIV
///                 ▲                ────────────▶│110  │
///                 │                      "none" │     │
///         CLKOUT 0 select          ────────────▶│111  │
///         CLKOUTSEL0[2:0]                       └─────┘
///                                                  ▲
///                                                  │
///                                          CLKOUT 1 select
///                                          CLKOUTSEL1[2:0]
/// ```
#[derive(Copy, Clone, Default, Debug)]
pub enum ClockOutSource {
    /// TODO: Doc comments
    M16Irc,
    ClkIn,
    M1Lposc,
    M4860Irc,
    MainClk,
    DspMainClk,
    MainPllClk,
    Aux0PllClk,
    DspPllClk,
    Aux1PllClk,
    AudioPllClk,
    K32Clk,
    #[default]
    None,
}

/// ```text
/// ┌────────────────────────────────────────────────────────────────────────────────────────┐
/// │                                                                                        │
/// │                                                          ┌─────────────┐               │
/// │                                                          │Main PLL     │  main_pll_clk │
/// │                                                   ┌─────▶│Clock Divider│ ────────────▶ │
/// │                                                   │      └─────────────┘               │
/// │                                                   │             ▲                      │
/// │                                                   │             │                      │
/// │                                                   │       MAINPLLCLKDIV                │
/// │                                                   │      ┌─────────────┐               │
/// │                                                   │      │DSP PLL      │   dsp_pll_clk │
/// │                                                   │ ┌───▶│Clock Divider│ ────────────▶ │
/// │                                                   │ │    └─────────────┘               │
/// │                     ┌─────┐       ┌────────────┐  │ │           ▲                      │
/// │         16m_irc ───▶│000  │       │       PFD0 │──┘ │           │                      │
/// │          clk_in ───▶│001  │       │ Main  PFD1 │────┘     DSPPLLCLKDIV                 │
/// │ 48/60m_irc_div2 ───▶│010  │──────▶│ PLL   PFD2 │────┐    ┌─────────────┐               │
/// │          "none" ───▶│111  │       │       PFD3 │───┐│    │AUX0 PLL     │  aux0_pll_clk │
/// │                     └─────┘       └────────────┘   │└───▶│Clock Divider│ ────────────▶ │
/// │                        ▲                 ▲         │     └─────────────┘               │
/// │                        │                 │         │            ▲                      │
/// │            Sys PLL clock select  Main PLL settings │            │                      │
/// │             SYSPLL0CLKSEL[2:0]       SYSPLL0xx     │      AUX0PLLCLKDIV                │
/// │                                                    │     ┌─────────────┐               │
/// │                                                    │     │AUX1 PLL     │  aux1_pll_clk │
/// │                                                    └────▶│Clock Divider│ ────────────▶ │
/// │                                                          └─────────────┘               │
/// │                                                                 ▲                      │
/// │                                                                 │                      │
/// │                                                           AUX1PLLCLKDIV                │
/// │                                                                                        │
/// └────────────────────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MainPll {
    /// Select the used clock input
    pub clock_select: MainPllClockSelect,
    /// Allowed range: 16..=22
    pub multiplier: u8,
    /// PFD that feeds the `main_pll_clk`.
    /// If None, the pfd is gated and the clock will not be active.
    ///
    /// Allowed range: `12..=35`.
    /// Applied multiplier = `18/div`.
    pub pfd0_div: Option<u8>,
    /// PFD that feeds the `dsp_pll_clk`.
    /// If None, the pfd is gated and the clock will not be active.
    ///
    /// Allowed range: `12..=35`.
    /// Applied multiplier = `18/div`.
    pub pfd1_div: Option<u8>,
    /// PFD that feeds the `aux0_pll_clk`.
    /// If None, the pfd is gated and the clock will not be active.
    ///
    /// Allowed range: `12..=35`.
    /// Applied multiplier = `18/div`.
    pub pfd2_div: Option<u8>,
    /// PFD that feeds the `aux1_pll_clk`.
    /// If None, the pfd is gated and the clock will not be active.
    ///
    /// Allowed range: `12..=35`.
    /// Applied multiplier = `18/div`.
    pub pfd3_div: Option<u8>,
    /// Clock divider for the `main_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub main_pll_clock_divider: u16,
    /// Clock divider for the `dsp_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub dsp_pll_clock_divider: u16,
    /// Clock divider for the `aux0_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub aux0_pll_clock_divider: u16,
    /// Clock divider for the `aux1_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub aux1_pll_clock_divider: u16,
}

/// ```text
///                     ┌─────┐
///         16m_irc ───▶│000  │
///          clk_in ───▶│001  │
/// 48/60m_irc_div2 ───▶│010  │──────▶
///          "none" ───▶│111  │
///                     └─────┘
///                        ▲
///                        │
///            Sys PLL clock select
///             SYSPLL0CLKSEL[2:0]
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainPllClockSelect {
    _16mIrc = 0b000,
    ClkIn = 0b001,
    _48_60MIrcDiv2 = 0b010,
}

/// ```text
/// clkin (selected by IOCON)          ┌───┐ clk_in
///       ────────────────────────────▶│1  │───────▶
///                ┌────────────┐  ┌──▶│0  │
///  xtalin ──────▶│Main crystal│  │   └───┘
/// xtalout ──────▶│oscillator  │──┘     ▲
///                └────────────┘        │
///                       ▲       SYSOSCBYPASS[2:0]
///                       │
///                Enable & bypass
///                SYSOSCCTL0[1:0]
/// ```
#[derive(Clone, Copy, Debug)]
pub enum ClkInSelect {
    Xtal { freq: u32, bypass: bool, low_power: bool },
    ClkIn0_25 { freq: u32, pin: PIO0_25 },
    ClkIn2_15 { freq: u32, pin: PIO2_15 },
    ClkIn2_30 { freq: u32, pin: PIO2_30 },
}

impl Default for ClkInSelect {
    fn default() -> Self {
        todo!()
    }
}

/// ```text
///                      ┌────┐
///  48/60m_irc_div2 ───▶│00  │
///           clk_in ───▶│01  │                      ┌────┐
///         1m_lposc ───▶│10  │─────────────────────▶│00  │
///       48/60m_irc ───▶│11  │         16m_irc ┌───▶│01  │
///                      └────┘    ─────────────┘┌──▶│10  │─────▶
///                         ▲      main_pll_clk  │┌─▶│11  │
///                         │      ──────────────┘│  └────┘
///           Main clock select A       32k_clk   │     ▲
///            MAINCLKSELA[1:0]    ───────────────┘     │
///                                       Main clock select B
///                                        MAINCLKSELB[1:0]
/// ```
// Top 2 bits = MAINCLKSELA
// Bottom 2 bits = MAINCLKSELB
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainClockSelect {
    _48_60MIrcDiv4 = 0b00_00,
    ClkIn = 0b01_00,
    _1mLposc = 0b10_00,
    _48_60MIrc = 0b11_00,
    _16mIrc = 0b00_01,
    MainPllClk = 0b0010,
    _32kClk = 0b0011,
}

/// ```text
///  ┌──────────┐                              48/60m_irc
///  │48/60 MHz │─┬──────────────────────────────────────▶
///  │Oscillator│ │ ┌───────────┐         48/60m_irc_div2
///  └──────────┘ └▶│Divide by 2│────────────────────────▶
///        ▲        └───────────┘         48/60m_irc_div4
///        │              │ ┌───────────┐ ┌──────────────▶
/// PDRUNCFG0[15],        └▶│Divide by 2│─┘
/// PDSLEEPCFG0[15]         └───────────┘
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum _48_60mIrcSelect {
    Off,
    Mhz48,
    Mhz60,
}

impl _48_60mIrcSelect {
    pub fn freq(&self) -> Option<u32> {
        match self {
            _48_60mIrcSelect::Off => None,
            _48_60mIrcSelect::Mhz48 => Some(48_000_000),
            _48_60mIrcSelect::Mhz60 => Some(60_000_000),
        }
    }

    /// Returns `true` if the  48 60m irc select is [`Off`].
    ///
    /// [`Off`]: _48_60mIrcSelect::Off
    #[must_use]
    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }
}

/// ```text
///                                                    1m_lposc
///                   ┌─────────────────────────────────────────▶
///                   │           32k_clk
///                   │           ───────┐
///  ┌──────────┐     │   ┌─────────┐    │  ┌─────┐
///  │1 MHz low │     │   │divide by│    └─▶│000  │ 32k_wake_clk
///  │power osc.│─────┴──▶│   32    │ ─────▶│001  │─────────────▶
///  └──────────┘         └─────────┘    ┌─▶│111  │
///        ▲                      "none" │  └─────┘
///        │                      ───────┘     ▲
/// PDRUNCFG0[14],                             │
/// PDSLEEPCFG0[14]                  WAKECLK32KHZSEL[2:0]
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum _32kWakeClkSelect {
    Off = 0b111,
    _32kClk = 0b000,
    Lp32k = 0b001,
}

impl _32kWakeClkSelect {
    /// Returns `true` if the  32k wake clk select is [`Off`].
    ///
    /// [`Off`]: _32kWakeClkSelect::Off
    #[must_use]
    pub fn is_off(&self) -> bool {
        matches!(self, Self::Off)
    }
}
