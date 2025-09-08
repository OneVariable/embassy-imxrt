use paste::paste;

use crate::iopctl::IopctlPin;
use crate::pac;
use crate::peripherals::{PIO0_25, PIO2_15, PIO2_30};

/// Max cpu freq is 300mhz, so that's 300 clock ticks per micro second
const WORST_CASE_TICKS_PER_US: u32 = 300;

#[derive(Debug, Clone, Default)]
pub struct Clocks {
    pub _1m_lposc: StaticClock<1_000_000>,
    pub _16m_irc: StaticClock<16_000_000>,
    pub _32k_clk: StaticClock<32_768>,
    pub _32k_wake_clk: StaticClock<32_768>,
    pub _48_60m_irc: Option<u32>,
    pub audio_pll_clk: Option<u32>,
    pub aux0_pll_clk: Option<u32>,
    pub aux1_pll_clk: Option<u32>,
    pub clk_in: Option<u32>,
    pub dsp_main_clk: Option<u32>,
    pub dsp_pll_clk: Option<u32>,
    // TODO(AJM): This is actually n=0..=7
    pub frg_clk_n: Option<u32>,
    pub frg_clk_14: Option<u32>,
    pub frg_clk_15: Option<u32>,
    pub frg_pll: Option<u32>,
    pub hclk: Option<u32>,
    pub lp_32k: StaticClock<32_768>,
    pub main_clk: u32,
    pub main_pll_clk: Option<u32>,
    pub mclk_in: Option<u32>,
    pub ostimer_clk: Option<u32>,
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct StaticClock<const F: u32> {
    pub enabled: bool,
}

impl<const F: u32> StaticClock<F> {
    fn as_option(self) -> Option<u32> {
        self.into()
    }
}

impl<const F: u32> From<StaticClock<F>> for Option<u32> {
    fn from(value: StaticClock<F>) -> Self {
        value.enabled.then_some(F)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClockConfig {
    /// Clock coming from RTC crystal oscillator
    pub enable_32k_clk: bool,
    pub enable_16m_irc: bool,
    pub enable_1m_lposc: bool,
    pub _48_60m_irc_select: _48_60mIrcSelect,
    pub _32k_wake_clk_select: _32kWakeClkSelect,
    pub clk_in_select: ClkInSelect,
    pub main_pll: Option<MainPll>,
    pub main_clock_select: MainClockSelect,
}

impl Default for ClockConfig {
    fn default() -> Self {
        todo!()
    }
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

pub enum ClockError {}

/// SAFETY: must be called exactly once at bootup
pub(crate) unsafe fn init(config: ClockConfig) -> Result<(), ClockError> {
    // TODO: When enabling clocks, wait the appropriate time

    let mut clocks = Clocks::default();

    let clkctl0 = pac::Clkctl0::steal();
    let clkctl1 = pac::Clkctl1::steal();
    let sysctl0 = pac::Sysctl0::steal();
    let sysctl1 = pac::Sysctl1::steal();

    // Optionally enable the clk_in
    match config.clk_in_select {
        ClkInSelect::Xtal {
            freq,
            bypass,
            low_power,
        } => {
            clkctl0
                .sysoscctl0()
                .write(|w| w.bypass_enable().bit(bypass).lp_enable().bit(low_power));
            clocks.clk_in = Some(freq);
        }
        ClkInSelect::ClkIn0_25 { freq, pin } => {
            pin.set_function(crate::gpio::Function::F7);
            clocks.clk_in = Some(freq);
        }
        ClkInSelect::ClkIn2_15 { freq, pin } => {
            pin.set_function(crate::gpio::Function::F7);
            clocks.clk_in = Some(freq);
        }
        ClkInSelect::ClkIn2_30 { freq, pin } => {
            pin.set_function(crate::gpio::Function::F5);
            clocks.clk_in = Some(freq);
        }
    }

    // Optionally enable the RTC 32k clock
    clkctl0
        .osc32khzctl0()
        .write(|w| w.ena32khz().bit(config.enable_32k_clk));
    clocks._32k_clk.enabled = config.enable_32k_clk;

    if !config._48_60m_irc_select.is_off() {
        // Select the 48/60m_irc clock speed
        clkctl0.ffroctl1().write(|w| w.update().update_safe_mode());
        let variant = match config._48_60m_irc_select {
            _48_60mIrcSelect::Off => unreachable!(),
            _48_60mIrcSelect::Mhz48 => pac::clkctl0::ffroctl0::TrimRange::Ffro48mhz,
            _48_60mIrcSelect::Mhz60 => pac::clkctl0::ffroctl0::TrimRange::Ffro60mhz,
        };
        clkctl0.ffroctl0().write(|w| w.trim_range().variant(variant));
        clkctl0.ffroctl1().write(|w| w.update().normal_mode());
    }

    // Optionally enable the 16m_irc, 48/60m_irc, 1m_lposc & lp_32k
    sysctl0.pdruncfg0().modify(|_, w| {
        w.sfro_pd().bit(!config.enable_16m_irc);
        w.lposc_pd().bit(!config.enable_1m_lposc);
        w.ffro_pd().bit(config._48_60m_irc_select.is_off());
        w
    });
    clocks._16m_irc.enabled = config.enable_16m_irc;
    clocks._1m_lposc.enabled = config.enable_1m_lposc;
    clocks.lp_32k.enabled = config.enable_1m_lposc;
    clocks._48_60m_irc = config._48_60m_irc_select.freq();

    // Optionally enable the 32k_wake_clk
    clkctl0
        .wakeclk32khzsel()
        .write(|w| w.sel().bits(config._32k_wake_clk_select as u8));
    clocks._32k_wake_clk.enabled = !config._32k_wake_clk_select.is_off();

    if let Some(main_pll) = config.main_pll {
        assert!(
            (16..=22).contains(&main_pll.multiplier),
            "main pll multiplier out of allowed range"
        );

        let pll_input_freq = match main_pll.clock_select {
            MainPllClockSelect::_16mIrc => clocks
                ._16m_irc
                .as_option()
                .expect("`_16m_irc` is not enabled, but the main PLL wants to use it"),
            MainPllClockSelect::ClkIn => clocks
                .clk_in
                .expect("`clk_in` is not enabled, but the main PLL wants to use it"),
            MainPllClockSelect::_48_60MIrcDiv2 => clocks
                ._48_60m_irc
                .expect("`_48_60m_irc` is not enabled, but the main PLL wants to use it"),
        };

        // Turn off the PLL if it was running
        sysctl0.pdruncfg0_set().write(|w| {
            w.syspllldo_pd().set_pdruncfg0();
            w.syspllana_pd().set_pdruncfg0();
            w
        });

        // Select the clock input we want
        clkctl0
            .syspll0clksel()
            .write(|w| w.sel().bits(main_pll.clock_select as u8));

        // Set the fractional part of the multiplier to 0
        // This means we're only using the integer multiplier as specified in the config
        clkctl0.syspll0num().write(|w| unsafe { w.num().bits(0x0) });
        clkctl0.syspll0denom().write(|w| unsafe { w.denom().bits(0x1) });

        clkctl0.syspll0ctl0().write(|w| {
            // No bypass. We're using the PFD.
            w.bypass().programmed_clk();
            // Clear the reset because after this we're fully configured
            w.reset().normal();
            // Set the user provided multiplier
            w.mult().bits(main_pll.multiplier);
            // For the first period we need the HOLDRINGOFF_ENA on
            w.holdringoff_ena().enable();
            w
        });

        // Turn on the PLL
        sysctl0.pdruncfg0_clr().write(|w| {
            w.syspllldo_pd().clr_pdruncfg0();
            w.syspllana_pd().clr_pdruncfg0();
            w
        });

        // Get the amount of us we need to wait
        let lock_time_div_2 = clkctl0.syspll0locktimediv2().read().locktimediv2().bits();
        cortex_m::asm::delay(WORST_CASE_TICKS_PER_US * lock_time_div_2 as u32);

        // For the second period we need the HOLDRINGOFF_ENA off
        clkctl0.syspll0ctl0().modify(|_, w| w.holdringoff_ena().dsiable());
        cortex_m::asm::delay(WORST_CASE_TICKS_PER_US * lock_time_div_2 as u32);

        // Output freq is just input times multiplier because the fractional part is hardcoded at 0
        let pll_output_freq = pll_input_freq * main_pll.multiplier as u32;

        // Main PLL is ready
        // Now we do the downstream PFDs

        // PFD0
        if let Some(div) = main_pll.pfd0_div {
            assert!((12..=35).contains(&div), "`pfd0_div` is out of the allowed range");

            clkctl0.syspll0pfd().modify(|_, w| {
                w.pfd0().bits(div);
                w.pfd0_clkrdy().set_bit();
                w.pfd0_clkgate().not_gated();
                w
            });
            while clkctl0.syspll0pfd().read().pfd0_clkrdy().bit_is_clear() {}

            let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

            assert!(
                (1..=256).contains(&main_pll.main_pll_clock_divider),
                "`main_pll_clock_divider` is out of the allowed range"
            );

            // Halt and reset the div
            clkctl0.mainpllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.mainpllclkdiv().write(|w| {
                w.div()
                    .bits((main_pll.main_pll_clock_divider - 1) as u8)
                    .reset()
                    .set_bit()
            });
            while clkctl0.mainpllclkdiv().read().reqflag().bit_is_set() {}

            clocks.main_pll_clk = Some(pfd_freq / (main_pll.main_pll_clock_divider - 1) as u32);
        } else {
            clkctl0.mainpllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.syspll0pfd().modify(|_, w| w.pfd0_clkgate().gated());
        }

        // PFD1
        if let Some(div) = main_pll.pfd1_div {
            assert!((12..=35).contains(&div), "`pfd1_div` is out of the allowed range");

            clkctl0.syspll0pfd().modify(|_, w| {
                w.pfd1().bits(div);
                w.pfd1_clkrdy().set_bit();
                w.pfd1_clkgate().not_gated();
                w
            });
            while clkctl0.syspll0pfd().read().pfd1_clkrdy().bit_is_clear() {}

            let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

            assert!(
                (1..=256).contains(&main_pll.dsp_pll_clock_divider),
                "`dsp_pll_clock_divider` is out of the allowed range"
            );

            // Halt and reset the div
            clkctl0.dsppllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.dsppllclkdiv().write(|w| {
                w.div()
                    .bits((main_pll.dsp_pll_clock_divider - 1) as u8)
                    .reset()
                    .set_bit()
            });
            while clkctl0.dsppllclkdiv().read().reqflag().bit_is_set() {}

            clocks.dsp_pll_clk = Some(pfd_freq / (main_pll.dsp_pll_clock_divider - 1) as u32);
        } else {
            clkctl0.dsppllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.syspll0pfd().modify(|_, w| w.pfd1_clkgate().gated());
        }

        // PFD2
        if let Some(div) = main_pll.pfd2_div {
            assert!((12..=35).contains(&div), "`pfd2_div` is out of the allowed range");

            clkctl0.syspll0pfd().modify(|_, w| {
                w.pfd2().bits(div);
                w.pfd2_clkrdy().set_bit();
                w.pfd2_clkgate().not_gated();
                w
            });
            while clkctl0.syspll0pfd().read().pfd2_clkrdy().bit_is_clear() {}

            let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

            assert!(
                (1..=256).contains(&main_pll.aux0_pll_clock_divider),
                "`aux0_pll_clock_divider` is out of the allowed range"
            );

            // Halt and reset the div
            clkctl0.aux0pllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.aux0pllclkdiv().write(|w| {
                w.div()
                    .bits((main_pll.aux0_pll_clock_divider - 1) as u8)
                    .reset()
                    .set_bit()
            });
            while clkctl0.aux0pllclkdiv().read().reqflag().bit_is_set() {}

            clocks.aux0_pll_clk = Some(pfd_freq / (main_pll.aux0_pll_clock_divider - 1) as u32);
        } else {
            clkctl0.aux0pllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.syspll0pfd().modify(|_, w| w.pfd2_clkgate().gated());
        }

        // PFD3
        if let Some(div) = main_pll.pfd3_div {
            assert!((12..=35).contains(&div), "`pfd3_div` is out of the allowed range");

            clkctl0.syspll0pfd().modify(|_, w| {
                w.pfd3().bits(div);
                w.pfd3_clkrdy().set_bit();
                w.pfd3_clkgate().not_gated();
                w
            });
            while clkctl0.syspll0pfd().read().pfd3_clkrdy().bit_is_clear() {}

            let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

            assert!(
                (1..=256).contains(&main_pll.aux1_pll_clock_divider),
                "`aux1_pll_clock_divider` is out of the allowed range"
            );

            // Halt and reset the div
            clkctl0.aux1pllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.aux1pllclkdiv().write(|w| {
                w.div()
                    .bits((main_pll.aux1_pll_clock_divider - 1) as u8)
                    .reset()
                    .set_bit()
            });
            while clkctl0.aux1pllclkdiv().read().reqflag().bit_is_set() {}

            clocks.aux1_pll_clk = Some(pfd_freq / (main_pll.aux1_pll_clock_divider - 1) as u32);
        } else {
            clkctl0.aux1pllclkdiv().write(|w| w.halt().set_bit());
            clkctl0.syspll0pfd().modify(|_, w| w.pfd3_clkgate().gated());
        }
    } else {
        // Turn off the PLL if it was running
        sysctl0.pdruncfg0_set().write(|w| {
            w.syspllldo_pd().set_pdruncfg0();
            w.syspllana_pd().set_pdruncfg0();
            w
        });

        clkctl0.syspll0clksel().write(|w| w.sel().none());
    }

    // Select the main clock
    clkctl0
        .mainclksela()
        .write(|w| w.bits((config.main_clock_select as u32 & 0b11_00) >> 2));
    clkctl0
        .mainclkselb()
        .write(|w| w.bits(config.main_clock_select as u32 & 0b00_11));

    clocks.main_clk = match config.main_clock_select {
        MainClockSelect::_48_60MIrcDiv4 => {
            clocks
                ._48_60m_irc
                .expect("Main clock uses _48_60m_irc, but _48_60m_irc is not active")
                / 4
        }
        MainClockSelect::ClkIn => clocks.clk_in.expect("Main clock uses clk_in, but clk_in is not active"),
        MainClockSelect::_1mLposc => clocks
            ._1m_lposc
            .as_option()
            .expect("Main clock uses _1m_lposc, but _1m_lposc is not active"),
        MainClockSelect::_48_60MIrc => clocks
            ._48_60m_irc
            .expect("Main clock uses _48_60m_irc, but _48_60m_irc is not active"),
        MainClockSelect::_16mIrc => clocks
            ._16m_irc
            .as_option()
            .expect("Main clock uses _16m_irc, but _16m_irc is not active"),
        MainClockSelect::MainPllClk => clocks
            .main_pll_clk
            .expect("Main clock uses main_pll_clk, but main_pll_clk is not active"),
        MainClockSelect::_32kClk => clocks
            ._32k_clk
            .as_option()
            .expect("Main clock uses _32k_clk, but _32k_clk is not active"),
    };

    todo!()
}

///Trait to expose perph clocks
trait SealedSysconPeripheral {
    fn enable_perph_clock();
    fn reset_perph();
    fn disable_perph_clock();
}

/// Clock and Reset control for peripherals
#[allow(private_bounds)]
pub trait SysconPeripheral: SealedSysconPeripheral + 'static {}

/// Enables and resets peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
pub fn enable_and_reset<T: SysconPeripheral>() {
    T::enable_perph_clock();
    T::reset_perph();
}

/// Enables peripheral `T`.
pub fn enable<T: SysconPeripheral>() {
    T::enable_perph_clock();
}

/// Reset peripheral `T`.
pub fn reset<T: SysconPeripheral>() {
    T::reset_perph();
}

/// Disables peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
pub fn disable<T: SysconPeripheral>() {
    T::disable_perph_clock();
}

pub fn clock_freq<T: SysconPeripheral>() -> u32 {
    todo!()
}

macro_rules! impl_perph_clk {
    ($peripheral:ident, $clkctl:ident, $clkreg:ident, $rstctl:ident, $rstreg:ident, $bit:expr) => {
        impl SealedSysconPeripheral for crate::peripherals::$peripheral {
            fn enable_perph_clock() {
                // SAFETY: unsafe needed to take pointers to Rstctl1 and Clkctl1
                let cc1 = unsafe { pac::$clkctl::steal() };

                paste! {
                    // SAFETY: unsafe due to the use of bits()
                    cc1.[<$clkreg _set>]().write(|w| unsafe { w.bits(1 << $bit) });
                }
            }

            fn reset_perph() {
                // SAFETY: unsafe needed to take pointers to Rstctl1 and Clkctl1
                let rc1 = unsafe { pac::$rstctl::steal() };

                paste! {
                    // SAFETY: unsafe due to the use of bits()
                    rc1.[<$rstreg _clr>]().write(|w| unsafe { w.bits(1 << $bit) });
                }
            }

            fn disable_perph_clock() {
                // SAFETY: unsafe needed to take pointers to Rstctl1 and Clkctl1
                let cc1 = unsafe { pac::$clkctl::steal() };

                paste! {
                    // SAFETY: unsafe due to the use of bits()
                    cc1.[<$clkreg _clr>]().write(|w| unsafe { w.bits(1 << $bit) });
                }
            }
        }

        impl SysconPeripheral for crate::peripherals::$peripheral {}
    };
}

// These should enabled once the relevant peripherals are implemented.
// impl_perph_clk!(GPIOINTCTL, Clkctl1, pscctl2, Rstctl1, prstctl2, 30);
// impl_perph_clk!(OTP, Clkctl0, pscctl0, Rstctl0, prstctl0, 17);

// impl_perph_clk!(ROM_CTL_128KB, Clkctl0, pscctl0, Rstctl0, prstctl0, 2);
// impl_perph_clk!(USBHS_SRAM, Clkctl0, pscctl0, Rstctl0, prstctl0, 23);

impl_perph_clk!(PIMCTL, Clkctl1, pscctl2, Rstctl1, prstctl2, 31);
impl_perph_clk!(ACMP, Clkctl0, pscctl1, Rstctl0, prstctl1, 15);
impl_perph_clk!(ADC0, Clkctl0, pscctl1, Rstctl0, prstctl1, 16);
impl_perph_clk!(CASPER, Clkctl0, pscctl0, Rstctl0, prstctl0, 9);
impl_perph_clk!(CRC, Clkctl1, pscctl1, Rstctl1, prstctl1, 16);
impl_perph_clk!(CTIMER0_COUNT_CHANNEL0, Clkctl1, pscctl2, Rstctl1, prstctl2, 0);
impl_perph_clk!(CTIMER1_COUNT_CHANNEL0, Clkctl1, pscctl2, Rstctl1, prstctl2, 1);
impl_perph_clk!(CTIMER2_COUNT_CHANNEL0, Clkctl1, pscctl2, Rstctl1, prstctl2, 2);
impl_perph_clk!(CTIMER3_COUNT_CHANNEL0, Clkctl1, pscctl2, Rstctl1, prstctl2, 3);
impl_perph_clk!(CTIMER4_COUNT_CHANNEL0, Clkctl1, pscctl2, Rstctl1, prstctl2, 4);
impl_perph_clk!(DMA0, Clkctl1, pscctl1, Rstctl1, prstctl1, 23);
impl_perph_clk!(DMA1, Clkctl1, pscctl1, Rstctl1, prstctl1, 24);
impl_perph_clk!(DMIC0, Clkctl1, pscctl0, Rstctl1, prstctl0, 24);

#[cfg(feature = "_espi")]
impl_perph_clk!(ESPI, Clkctl0, pscctl1, Rstctl0, prstctl1, 7);

impl_perph_clk!(FLEXCOMM0, Clkctl1, pscctl0, Rstctl1, prstctl0, 8);
impl_perph_clk!(FLEXCOMM1, Clkctl1, pscctl0, Rstctl1, prstctl0, 9);
impl_perph_clk!(FLEXCOMM14, Clkctl1, pscctl0, Rstctl1, prstctl0, 22);
impl_perph_clk!(FLEXCOMM15, Clkctl1, pscctl0, Rstctl1, prstctl0, 23);
impl_perph_clk!(FLEXCOMM2, Clkctl1, pscctl0, Rstctl1, prstctl0, 10);
impl_perph_clk!(FLEXCOMM3, Clkctl1, pscctl0, Rstctl1, prstctl0, 11);
impl_perph_clk!(FLEXCOMM4, Clkctl1, pscctl0, Rstctl1, prstctl0, 12);
impl_perph_clk!(FLEXCOMM5, Clkctl1, pscctl0, Rstctl1, prstctl0, 13);
impl_perph_clk!(FLEXCOMM6, Clkctl1, pscctl0, Rstctl1, prstctl0, 14);
impl_perph_clk!(FLEXCOMM7, Clkctl1, pscctl0, Rstctl1, prstctl0, 15);
impl_perph_clk!(FLEXSPI, Clkctl0, pscctl0, Rstctl0, prstctl0, 16);
impl_perph_clk!(FREQME, Clkctl1, pscctl1, Rstctl1, prstctl1, 31);
impl_perph_clk!(HASHCRYPT, Clkctl0, pscctl0, Rstctl0, prstctl0, 10);
impl_perph_clk!(HSGPIO0, Clkctl1, pscctl1, Rstctl1, prstctl1, 0);
impl_perph_clk!(HSGPIO1, Clkctl1, pscctl1, Rstctl1, prstctl1, 1);
impl_perph_clk!(HSGPIO2, Clkctl1, pscctl1, Rstctl1, prstctl1, 2);
impl_perph_clk!(HSGPIO3, Clkctl1, pscctl1, Rstctl1, prstctl1, 3);
impl_perph_clk!(HSGPIO4, Clkctl1, pscctl1, Rstctl1, prstctl1, 4);
impl_perph_clk!(HSGPIO5, Clkctl1, pscctl1, Rstctl1, prstctl1, 5);
impl_perph_clk!(HSGPIO6, Clkctl1, pscctl1, Rstctl1, prstctl1, 6);
impl_perph_clk!(HSGPIO7, Clkctl1, pscctl1, Rstctl1, prstctl1, 7);
impl_perph_clk!(I3C0, Clkctl1, pscctl2, Rstctl1, prstctl2, 16);
impl_perph_clk!(MRT0, Clkctl1, pscctl2, Rstctl1, prstctl2, 8);
impl_perph_clk!(MU_A, Clkctl1, pscctl1, Rstctl1, prstctl1, 28);
impl_perph_clk!(OS_EVENT, Clkctl1, pscctl0, Rstctl1, prstctl0, 27);
impl_perph_clk!(POWERQUAD, Clkctl0, pscctl0, Rstctl0, prstctl0, 8);
impl_perph_clk!(PUF, Clkctl0, pscctl0, Rstctl0, prstctl0, 11);
impl_perph_clk!(RNG, Clkctl0, pscctl0, Rstctl0, prstctl0, 12);
impl_perph_clk!(RTC, Clkctl1, pscctl2, Rstctl1, prstctl2, 7);
impl_perph_clk!(SCT0, Clkctl0, pscctl0, Rstctl0, prstctl0, 24);
impl_perph_clk!(SECGPIO, Clkctl0, pscctl1, Rstctl0, prstctl1, 24);
impl_perph_clk!(SEMA42, Clkctl1, pscctl1, Rstctl1, prstctl1, 29);
impl_perph_clk!(USBHSD, Clkctl0, pscctl0, Rstctl0, prstctl0, 21);
impl_perph_clk!(USBHSH, Clkctl0, pscctl0, Rstctl0, prstctl0, 22);
impl_perph_clk!(USBPHY, Clkctl0, pscctl0, Rstctl0, prstctl0, 20);
impl_perph_clk!(USDHC0, Clkctl0, pscctl1, Rstctl0, prstctl1, 2);
impl_perph_clk!(USDHC1, Clkctl0, pscctl1, Rstctl0, prstctl1, 3);
impl_perph_clk!(UTICK0, Clkctl0, pscctl2, Rstctl0, prstctl2, 0);
impl_perph_clk!(WDT0, Clkctl0, pscctl2, Rstctl0, prstctl2, 1);
impl_perph_clk!(WDT1, Clkctl1, pscctl2, Rstctl1, prstctl2, 10);

// Diagrams without homes (yet)
//
// -----
//
//             ┌───────────┐
//  rtcxin ───▶│RTC crystal│ 32k_clk
// rtcxout ───▶│oscillator │─────────▶
//             └───────────┘
//                   ▲
//                   │
//                Enable
//            OSC23KHZCTL0[0]
//
// -----
//
//  ┌──────────┐
//  │16 MHz    │ 16m_irc
//  │oscillator│────────▶
//  └──────────┘
//        ▲
//        │
// PDRUNCFG0[14],
// PDSLEEPCFG0[14]
//
// -----
//
//                     ┌─────┐       ┌────────────┐
//         16m_irc ───▶│000  │       │            │      ┌─────────────┐
//          clk_in ───▶│001  │       │ Audio      │      │Main PLL     │  main_pll_clk
// 48/60m_irc_div2 ───▶│010  │──────▶│ PLL   PFD0 │─────▶│Clock Divider│ ────────────▶
//          "none" ───▶│111  │       │            │      └─────────────┘
//                     └─────┘       └────────────┘             ▲
//                        ▲                 ▲                   │
//                        │                 │             MAINPLLCLKDIV
//          Audio PLL clock select  Audio PLL settings
//           AUDIOPLL0CLKSEL[2:0]        SYSPLL0xx
//
// -----
//
// (first half partially used above)
//
//                                                               ┌─────────┐
//                                                               │CPU Clock│  to CPU, AHB, APB, etc.
//                                                          ┌───▶│Divider  │──────────────────────────────────────▶
//                                                          │    └─────────┘           hclk
//                     ┌────┐                               │         ▲
// 48/60m_irc_div2 ───▶│00  │                               │         │
//          clk_in ───▶│01  │                      ┌────┐   │  SYSCPUAHBCLKDIV
//        1m_lposc ───▶│10  │─────────────────────▶│00  │   │  ┌─────────────┐
//      48/60m_irc ───▶│11  │         16m_irc ┌───▶│01  │   │  │ARM Trace    │ to ARM Trace function clock
//                     └────┘    ─────────────┘┌──▶│10  │───┼─▶│Clock Divider│────────────────────────────────────▶
//                        ▲      main_pll_clk  │┌─▶│11  │   │  └─────────────┘
//                        │      ──────────────┘│  └────┘   │         ▲
//          Main clock select A       32k_clk   │     ▲     │         │
//           MAINCLKSELA[1:0]    ───────────────┘     │     │      PFC0DIV
//                                      Main clock select B │  ┌─────────────┐
//                                       MAINCLKSELB[1:0]   │  │Systick Clock│                ┌─────┐
//                                                          ├─▶│Divider      │───────────────▶│000  │
//                                                          │  └─────────────┘   1m_lposc┌───▶│001  │  to Systick
//                                                          │         ▲          ────────┘┌──▶│010  │─────────────▶
//                                                          │         │           32k_clk │┌─▶│011  │function clock
//                                                          │  SYSTICKFCLKDIV    ─────────┘│┌▶│111  │
//                                                          │                     16m_irc  ││ └─────┘
//                                                          ▼                    ──────────┘│    ▲
//                                                      main_clk                   "none"   │    │
//                                                                               ───────────┘    │
//                                                                                               │
//                                                                               SYSTICKFCLKSEL[2:0]
//
// -----
//
//                ┌────┐
// 48/60m_irc ───▶│00  │
//     clk_in ───▶│01  │                      ┌────┐
//   1m_lposc ───▶│10  │─────────────────────▶│00  │      ┌─────────┐
//    16m_irc ───▶│11  │    main_pll_clk ┌───▶│01  │      │DSP Clock│                       to DSP CPU
//                └────┘    ─────────────┘┌──▶│10  │───┬─▶│Divider  │──┬──────────────────────────────▶
//                   ▲       dsp_pll_clk  │┌─▶│11  │   │  └─────────┘  │  ┌─────────────┐
//                   │      ──────────────┘│  └────┘   │       ▲       │  │DSP RAM      │   to DSP RAM
//     DSP clock select A        32k_clk   │     ▲     │       │       └─▶│Clock Divider│─────────────▶
//     DSPCPUCLKSELA[1:0]   ───────────────┘     │     │ DSPCPUCLKDIV     └─────────────┘   interface
//                                 DSP clock select B  │                        ▲
//                                 DSPCPUCLKSELB[1:0]  │                        │
//                                                     ▼                DSPMAINRAMCLKDIV
//                                             dsp_main_clk (to
//                                             CLKOUT 0 select)
//
// -----
//
//               ┌────────────────┐
// main_pll_clk  │PLL to Flexcomm │ frg_pll
// ─────────────▶│FRG Divider     │────────▶
//               └────────────────┘
//                        ▲
//                        │
//                  FRGPLLCLKDIV
//
// -----
//
// 1 per Flexcomm interface (n = 0 through 7)
//
//                                          16m_clk_irc ┌─────┐
//                                           ──────────▶│000  │
//                                           48/60m_irc │     │
//                                           ──────────▶│001  │
//   main_clk ┌─────┐                     audio_pll_clk │     │
// ──────────▶│000  │                        ──────────▶│010  │ function clock
//    frg_pll │     │                           mclk_in │     │────────────────▶
// ──────────▶│001  │     ┌───────────────┐  ──────────▶│011  │  of Flexcomm n
//    16m_irc │     │     │Fractional Rate│   frg_clk n │     │
// ──────────▶│010  │────▶│Divider (FRG)  │────────────▶│100  │
// 48/60m_irc │     │     └───────────────┘       "none"│     │
// ──────────▶│011  │             ▲          ──────────▶│111  │
//     "none" │     │             │                     └─────┘
// ──────────▶│111  │       FRGnCTL[15:0]                  ▲
//            └─────┘                                      │
//               ▲                             Flexcomm n clock select
//               │                                 FCnFCLKSEL[2:0]
//       FRG clock select n
//        FRGnCLKSEL[2:0]
//
// -----
//
//
//                                          16m_clk_irc ┌─────┐
//                                           ──────────▶│000  │
//                                           48/60m_irc │     │
//                                           ──────────▶│001  │
//   main_clk ┌─────┐                     audio_pll_clk │     │
// ──────────▶│000  │                        ──────────▶│010  │ function clock
//    frg_pll │     │                           mclk_in │     │────────────────▶
// ──────────▶│001  │     ┌───────────────┐  ──────────▶│011  │    of HS SPI
//    16m_irc │     │     │Fractional Rate│   frg_clk14 │     │
// ──────────▶│010  │────▶│Divider (FRG)  │────────────▶│100  │
// 48/60m_irc │     │     └───────────────┘       "none"│     │
// ──────────▶│011  │             ▲          ──────────▶│111  │
//     "none" │     │             │                     └─────┘
// ──────────▶│111  │       FRG14CTL[15:0]                 ▲
//            └─────┘                                      │
//               ▲                               HS SPI clock select
//               │                                 FC14FCLKSEL[2:0]
//    HS SPI FRG clock select
//       FRG14CLKSEL[2:0]
//
// -----
//
//
//                                          16m_clk_irc ┌─────┐
//                                           ──────────▶│000  │
//                                           48/60m_irc │     │
//                                           ──────────▶│001  │
//   main_clk ┌─────┐                     audio_pll_clk │     │
// ──────────▶│000  │                        ──────────▶│010  │ function clock
//    frg_pll │     │                           mclk_in │     │────────────────▶
// ──────────▶│001  │     ┌───────────────┐  ──────────▶│011  │   of PMIC I2C
//    16m_irc │     │     │Fractional Rate│   frg_clk14 │     │
// ──────────▶│010  │────▶│Divider (FRG)  │────────────▶│100  │
// 48/60m_irc │     │     └───────────────┘       "none"│     │
// ──────────▶│011  │             ▲          ──────────▶│111  │
//     "none" │     │             │                     └─────┘
// ──────────▶│111  │       FRG15CTL[15:0]                 ▲
//            └─────┘                                      │
//               ▲                              PMIC I2C clock select
//               │                                 FC15FCLKSEL[2:0]
//   PMIC I2C FRG clock select
//       FRG15CLKSEL[2:0]
//
// -----
//
// 48/60m_irc ┌─────┐
// ──────────▶│000  │ to eSPI
//     "none" │     │────────▶
// ──────────▶│111  │   fclk
//            └─────┘
//               ▲
//               │
//       eSPI clock select
//       ESPIFCLKSEL[2:0]
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
// main_pll_clk │     │
// ────────────▶│001  │
// aux0_pll_clk │     │        ┌─────────────┐
// ────────────▶│010  │        │FlexSPI Clock│ to FlexSPI
//   48/60m_irc │     │───────▶│Divider      │───────────▶
// ────────────▶│011  │        └─────────────┘    fclk
// aux1_pll_clk │     │               ▲
// ────────────▶│100  │               │
//        "none"│     │        FLEXSPIFCLKDIV
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//        OSPI clock select
//        OSPIFFCLKSEL[2:0]
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
// main_pll_clk │     │
// ────────────▶│001  │
// aux0_pll_clk │     │        ┌─────────────┐
// ────────────▶│010  │        │SDIO0 Clock  │  to SDIO0
//   48/60m_irc │     │───────▶│Divider      │───────────▶
// ────────────▶│011  │        └─────────────┘    fclk
// aux1_pll_clk │     │               ▲
// ────────────▶│100  │               │
//        "none"│     │         SDIO0FCLKDIV
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//       SDIO 0 clock select
//        SDIO0FCLKSEL[2:0]
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
// main_pll_clk │     │
// ────────────▶│001  │
// aux0_pll_clk │     │        ┌─────────────┐
// ────────────▶│010  │        │SDIO1 Clock  │  to SDIO1
//   48/60m_irc │     │───────▶│Divider      │───────────▶
// ────────────▶│011  │        └─────────────┘    fclk
// aux1_pll_clk │     │               ▲
// ────────────▶│100  │               │
//        "none"│     │         SDIO1FCLKDIV
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//       SDIO 1 clock select
//        SDIO1FCLKSEL[2:0]
//
// -----
//
//      clk_in  ┌─────┐
// ────────────▶│000  │        ┌─────────────┐
//    main_clk  │     │        │USB Clock    │ to HS
// ────────────▶│001  │───────▶│Divider      │──────▶
//      "none"  │     │        └─────────────┘  USB
// ────────────▶│111  │               ▲
//              └─────┘               │
//                 ▲            USBHSFCLKDIV
//                 │
//         USB clock select
//        USBHSFCLKSEL[2:0]
//
// -----
//
//           ┌─────────────┐  to USB PHY
//  main_clk │USB PHY bus  │ bus interface
// ─────────▶│Clock Divider│───────────────▶
//           └─────────────┘ (max 120MHz)
//                  ▲
//                  │
//          CLKCTL0_PFC1DIV
//
// -----
//
//     main_clk ┌─────┐
// ────────────▶│000  │                             ┌────────┐
//   48/60m_irc │     │                             │I3C fclk│     to I3C fclk
// ────────────▶│001  │────────────┬───────────────▶│Divider │────────────────────▶
//       "none" │     │            │                └────────┘ mult of 24 or 25MHz
// ────────────▶│111  │            │    ┌─────┐          ▲
//              └─────┘            └───▶│000  │          │
//                 ▲                    │     │     I3C0FCLKDIV
//                 │         ┌─────────▶│001  │──┐  ┌────────┐
//         I3C clock select  │   "none" │     │  └─▶│I3C TC  │            to I3C
//         I3C0FCLKSEL[2:0]  │   ──────▶│111  │     │Divider │────────────────────▶
//                           │          └─────┘     └────────┘         clk_slow_tc
//                           │             ▲             ▲
//                           │             │             │
//                           │    I3C TC Select    I3C0FCLKSTCDIV
//                           │ I3C0FCLKSTCSEL[2:0]  ┌────────┐
//  1m_lposc                 │                      │I3C TC  │             to I3C
// ──────────────────────────┴─────────────────────▶│Divider │────────────────────▶
//                                                  └────────┘            clk_slow
//                                                       ▲
//                                                       │
//                                                  I3C0FCLKSDIV
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
// main_pll_clk │     │
// ────────────▶│001  │
// aux0_pll_clk │     │
// ────────────▶│010  │        ┌─────────────┐
//   48/60m_irc │     │        │SCTimer/PWM  │ to SCTimer/PWM
// ────────────▶│011  │───────▶│Clock Divider│───────────────▶
// aux1_pll_clk │     │        └─────────────┘  input clock 7
// ────────────▶│100  │               ▲
//audio_pll_clk │     │               │
// ────────────▶│101  │           SCTFCLKDIV
//       "none" │     │
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//     SCTimer/PWM clock select
//         SCTFCLKSEL[2:0]
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
//      16m_irc │     │
// ────────────▶│001  │
//   48/60m_irc │     │ function clock of
// ────────────▶│010  │     CTIMER n
//audio_pll_clk │     │──────────────────▶
// ────────────▶│011  │ CTIMERs 0 thru 4
//      mclk_in │     │
// ────────────▶│100  │
//       "none" │     │
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//       TIMER n clock select
//       CT32BITnFCLKSEL[2:0]
//
// -----
//
//     1m_lposc ┌─────┐
// ────────────▶│000  │
//      32k_clk │     │
// ────────────▶│001  │ ostimer_clk
//         hclk │     │────────────▶
// ────────────▶│010  │
//       "none" │     │
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//      OS Timer Clock Select
//       OSEVENTTFCLKSEL[2:0]
//
// -----
//
//   1m_lposc ┌─────┐
// ──────────▶│000  │ to WDT0
//     "none" │     │────────▶
// ──────────▶│111  │   fclk
//            └─────┘
//               ▲
//               │
//       WDT0 Clock Select
//       WDT0FCLKSEL[2:0]
//
// -----
//
//
//   1m_lposc ┌─────┐
// ──────────▶│000  │ to WDT1
//     "none" │     │────────▶
// ──────────▶│111  │   fclk
//            └─────┘
//               ▲
//               │
//       WDT1 Clock Select
//       WDT1FCLKSEL[2:0]
//
// -----
//
//
//   1m_lposc ┌─────┐
// ──────────▶│000  │ to UTICK
//     "none" │     │────────▶
// ──────────▶│111  │   fclk
//            └─────┘
//               ▲
//               │
//   Utick Timer Clock Select
//       UTICKFCLKSEL[2:0]
//
// -----
//
//
//    16m_irc ┌─────┐                      ┌─────┐
// ──────────▶│000  │    ┌────────────────▶│000  │
//     clk_in │     │    │     main_pll_clk│     │
// ──────────▶│001  │    │      ──────────▶│001  │        ┌─────────┐
//   1m_lposc │     │    │     aux0_pll_clk│     │        │ADC Clock│ to ADC
// ──────────▶│010  │────┘      ──────────▶│010  │───────▶│Divider  │───────▶
// 48/60m_irc │     │          aux1_pll_clk│     │        └─────────┘  fclk
// ──────────▶│011  │           ──────────▶│011  │             ▲
//    "none"  │     │              "none"  │     │             │
// ──────────▶│111  │           ──────────▶│111  │       ADC0FCLKDIV
//            └─────┘                      └─────┘
//               ▲                            ▲
//               │                            │
//      ADC clock select 0           ADC clock select 1
//       ADC0FCLKSEL0[2:0]            ADC0FCLKSEL1[2:0]
//
// -----
//
//     main clk ┌─────┐
// ────────────▶│000  │
//      16m_irc │     │
// ────────────▶│001  │
//   48/60m_irc │     │   ┌──────────┐
// ────────────▶│010  │   │ACMP Clock│ to ACMP
// aux0_pll_clk │     │──▶│Divider   │────────▶
// ────────────▶│011  │   └──────────┘   fclk
// aux1_pll_clk │     │        ▲
// ────────────▶│100  │        │
//       "none" │     │  ACMP0FCLKDIV
// ────────────▶│111  │
//              └─────┘
//                 ▲
//                 │
//        ACMP clock select
//        ACMP0FCLKSEL[2:0]
//
// -----
//
//       16m_irc ┌─────┐
//  ────────────▶│000  │
//    48/60m_irc │     │
//  ────────────▶│001  │
// audio_pll_clk │     │
//  ────────────▶│010  │      ┌──────────┐
//       mclk_in │     │      │DMIC Clock│ to D-Mic
//  ────────────▶│011  │─────▶│Divider   │─────────▶
//      1m_lposc │     │      └──────────┘subsystem
//  ────────────▶│100  │           ▲
//  32k_wake_clk │     │           │
//  ────────────▶│101  │     DMIC0FCLKDIV
//        "none" │     │
//  ────────────▶│111  │
//               └─────┘
//                  ▲
//                  │
//         DMIC Clock Select
//         DMIC0FCLKSEL[2:0]
//
// -----
//
//      16m_irc ┌─────┐                          ┌─────┐
// ────────────▶│000  │      ┌──────────────────▶│000  │
//       clk_in │     │      │      main_pll_clk │     │
// ────────────▶│001  │      │      ────────────▶│001  │
//     1m_lposc │     │      │      aux0_pll_clk │     │
// ────────────▶│010  │      │      ────────────▶│010  │
//   48/60m_irc │     │──────┘       dsp_pll_clk │     │    ┌───────┐
// ────────────▶│011  │             ────────────▶│011  │    │CLKOUT │    CLKOUT
//     main_clk │     │             aux1_pll_clk │     │───▶│Divider│────────────▶
// ────────────▶│100  │             ────────────▶│100  │    └───────┘
// dsp_main_clk │     │            audio_pll_clk │     │        ▲
// ────────────▶│110  │             ────────────▶│101  │        │
//              └─────┘                  32k_clk │     │    CLKOUTDIV
//                 ▲                ────────────▶│110  │
//                 │                      "none" │     │
//         CLKOUT 0 select          ────────────▶│111  │
//         CLKOUTSEL0[2:0]                       └─────┘
//                                                  ▲
//                                                  │
//                                          CLKOUT 1 select
//                                          CLKOUTSEL1[2:0]
//
// -----
