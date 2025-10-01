use core::cell::RefCell;

use config::{
    ClkInSelect, ClockConfig, ClockOutSource, MainClockSelect, MainPll, MainPllClockSelect, _32kWakeClkSelect,
    _48_60mIrcSelect,
};
use critical_section::Mutex;
use paste::paste;
use periph_helpers::{
    AdcConfig, CtimerConfig, FlexcommConfig, FlexcommConfig14, FlexcommConfig15, NoConfig, OsEventConfig, SPConfHelper,
    Sct0Config, WdtConfig,
};

use crate::iopctl::IopctlPin;
use crate::pac;

pub mod config;
pub(crate) mod periph_helpers;

/// Max cpu freq is 300mhz, so that's 300 clock ticks per micro second
const WORST_CASE_TICKS_PER_US: u32 = 300;
static CLOCKS: Mutex<RefCell<Option<Clocks>>> = Mutex::new(RefCell::new(None));

/// `init` is the main entrypoint for configuring clocks.
///
/// This should be called once on startup.
///
/// Once this function has completed successfully, all of the "root" clocks of the
/// system have been configured as specified in [`ClockConfig`], and are not currently
/// reconfigurable.
///
/// This state is stored in a write-once static in this module, and can be later
/// accessed in one of two ways:
///
/// 1. Via the [`with_clocks`] method, which gives read-only access to [`Clocks`]
/// 2. Individual peripheral source clocks are returned by calls to [`enable_and_reset`]
pub(crate) fn init(config: ClockConfig, clk_in_select: ClkInSelect) -> Result<(), ClockError> {
    // TODO: When enabling clocks, wait the appropriate time
    //
    // Ensure we haven't already configured the clocks
    critical_section::with(|cs| {
        if CLOCKS.borrow_ref(cs).is_some() {
            Err(ClockError::AlreadyConfigured)
        } else {
            Ok(())
        }
    })?;
    let mut clocks = Clocks::default();

    let mut operator = ClockOperator {
        config: &config,
        clocks: &mut clocks,
        clkctl0: unsafe { pac::Clkctl0::steal() },
        sysctl0: unsafe { pac::Sysctl0::steal() },
        clkctl1: unsafe { pac::Clkctl1::steal() },
        // sysctl1: pac::Sysctl1::steal(),
    };

    // If necessary, set up the clock-in
    operator.setup_clock_in(clk_in_select);

    // Set up the main pll, if requested
    let pll_output_freq = operator.setup_main_pll(config.main_pll)?;
    // Set up downstream plls, if requested
    if let Some((freq, main_pll)) = pll_output_freq {
        operator.setup_pll_pfd0(&main_pll, freq)?;
        operator.setup_pll_pfd1(&main_pll, freq)?;
        operator.setup_pll_pfd2(&main_pll, freq)?;
        operator.setup_pll_pfd3(&main_pll, freq)?;
    } else {
        // If the PLL output is not enabled, then we can't feed PFD0-3, so just
        // gate them to save power.
        operator.disable_pll_pfd0();
        operator.disable_pll_pfd1();
        operator.disable_pll_pfd2();
        operator.disable_pll_pfd3();
    }

    // Setup main clock
    operator.setup_main_clock()?;
    // Setup system clock
    operator.setup_system_clock()?;
    // Setup frg_pll_div
    operator.setup_frg_pll_div()?;

    // Ensure some core clocks are enabled, as we can't predict now what
    // other peripherals will want later. Ignore errors if not indicated
    // by the config.
    //
    // If the clocks have already been enabled, this is a nop.
    // If the clocks haven't been enabled, but aren't set in config:
    //  we'll ignore the err.
    // If the clocks haven't been enabled, but are set in the config:
    //  we'll enable them now, as long as their inputs are active.
    _ = operator.ensure_1mhz_lposc_active();
    _ = operator.ensure_48_60mhz_irc_active();
    _ = operator.ensure_16mhz_irc_active();
    _ = operator.ensure_main_pll_clk_active();
    _ = operator.ensure_32kclk_active();

    // Optionally enable the 32k_wake_clk
    operator.setup_32k_wake_clk()?;

    // Finally, setup clk_out
    operator.setup_clock_out()?;

    // Store the configured clocks object statically so we can retrieve it to
    // check coherency later
    critical_section::with(|cs| {
        *CLOCKS.borrow_ref_mut(cs) = Some(clocks);
    });
    Ok(())
}

/// STATE AFTER INIT: EXPLAIN
#[derive(Debug, Clone, Default)]
pub struct Clocks {
    /// "LPOSC", a very low power (but less accurate, +/- 10%) clock
    /// running at 1MHz
    pub _1m_lposc: StaticClock<1_000_000>,
    /// "SFRO", a higher power, +/- 1%, 16-MHz internal oscillator clock
    /// source (note: Datasheet says +/-3%, but reference manual mentions
    /// +/- 1%?)
    pub _16m_irc: StaticClock<16_000_000>,
    /// 32kHz RTC Crystal Oscillator
    pub _32k_clk: StaticClock<32_768>,
    /// 32kHz "wake clock", can be sourced from either the RTC oscillator,
    /// or a divided version of the 1m_lposc
    pub _32k_wake_clk: Option<u32>,
    /// "FFRO", a higher power, +/- 1%, 48- or 60-MHz internal oscillator
    /// clock source
    pub _48_60m_irc: Option<u32>,
    /// Output of the Main PLL (as PFD0), divided by the
    /// Main PLL Clock Divider
    pub main_pll_clk: Option<u32>,
    /// Output of the Main PLL (as PFD1), divided by the
    /// DSP PLL Clock Divider
    pub dsp_pll_clk: Option<u32>,
    /// Output of the Main PLL (as PFD2), divided by the
    /// AUX0 PLL Clock Divider
    pub aux0_pll_clk: Option<u32>,
    /// Output of the Main PLL (as PFD3), divided by the
    /// AUX1 PLL Clock Divider
    pub aux1_pll_clk: Option<u32>,
    /// External clock-in source, fed either by a clock source
    /// or external oscillator
    pub clk_in: Option<u32>,
    /// "Main Clock" sourced from a variety of selectable sources,
    /// used as the input for the CPU Clock Divider, ARM Trace Clock,
    /// and Systick clock source
    pub main_clk: u32,
    /// The output of the CPU Clock Divider, sourced from `main_clk`
    /// Also "hclk"
    pub sys_cpu_ahb_clk: Option<u32>,
    /// The output of FRGPLLCLKDIV, fed by main_pll_clk
    pub frg_pll_clk: Option<u32>,
    /// The clk_out output
    pub clk_out: Option<u32>,
    //
    // --- These clocks have not been configured yet ---
    // // We probably SHOULD (allow for) configuration of these clocks:
    //
    // pub dsp_main_clk: Option<u32>,
    // pub audio_pll_clk: Option<u32>,
    // /// The MCLK input function, when it is connected to a pin by selecting it in the IOCON block. May
    // /// be used as the function clock of any Flexcomm Interface, and/or to clock the DMIC peripheral.
    // pub mclk_in: Option<u32>,
    // /// This is the 1mlposc /32 (which is actually 31250Hz?), which can
    // /// be used for the _32k_wake_clk
    // pub lp_32k: StaticClock<32_768>,
    //
    // // These are more peripheral clocks
    //
    // // Clock after flexcomm-specific fractional rate generator, not handled by
    // // init, instead handled by periph drivers
    // pub frg_clk_n: [Option<u32>; FLEXCOMM_INSTANCES],
    // pub frg_clk_14: Option<u32>,
    // pub frg_clk_15: Option<u32>,
    //
    // /// Clock for the OS Event Timer peripheral.
    // pub ostimer_clk: Option<u32>,
}

impl Clocks {
    fn ensure_1m_lposc(&self) -> Result<u32, ClockError> {
        self._1m_lposc
            .as_option()
            .ok_or_else(|| ClockError::bad_config("1m_irc/lposc needed but not enabled"))
    }

    fn ensure_16m_sfro(&self) -> Result<u32, ClockError> {
        self._16m_irc
            .as_option()
            .ok_or_else(|| ClockError::bad_config("16m_irc/sfro needed but not enabled"))
    }

    fn ensure_48_60_ffro(&self) -> Result<u32, ClockError> {
        self._48_60m_irc
            .ok_or_else(|| ClockError::bad_config("48/60m_irc/ffro needed but not enabled"))
    }

    fn ensure_dsp_pll(&self) -> Result<u32, ClockError> {
        self.dsp_pll_clk
            .ok_or_else(|| ClockError::bad_config("dsp pll needed but not enabled"))
    }

    fn ensure_aux0_pll(&self) -> Result<u32, ClockError> {
        self.aux0_pll_clk
            .ok_or_else(|| ClockError::bad_config("aux0 pll needed but not enabled"))
    }

    fn ensure_aux1_pll(&self) -> Result<u32, ClockError> {
        self.aux1_pll_clk
            .ok_or_else(|| ClockError::bad_config("aux1 pll needed but not enabled"))
    }

    fn ensure_frg_pll(&self) -> Result<u32, ClockError> {
        self.frg_pll_clk
            .ok_or_else(|| ClockError::bad_config("frg pll needed but not enabled"))
    }

    fn ensure_xtal_in(&self) -> Result<u32, ClockError> {
        self.clk_in
            .ok_or_else(|| ClockError::bad_config("xtal_in needed but not enabled"))
    }

    fn ensure_32k_clk(&self) -> Result<u32, ClockError> {
        self._32k_clk
            .as_option()
            .ok_or_else(|| ClockError::bad_config("32k_clk needed but not enabled"))
    }

    fn ensure_hclk(&self) -> Result<u32, ClockError> {
        self.sys_cpu_ahb_clk
            .ok_or_else(|| ClockError::bad_config("hclk needed but not enabled"))
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct StaticClock<const F: u32> {
    pub enabled: bool,
}

impl<const F: u32> StaticClock<F> {
    fn as_option(self) -> Option<u32> {
        self.into()
    }

    fn frequency(&self) -> u32 {
        F
    }
}

impl<const F: u32> From<StaticClock<F>> for Option<u32> {
    fn from(value: StaticClock<F>) -> Self {
        value.enabled.then_some(F)
    }
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug)]
pub enum ClockError {
    /// The requested configuration was impossible or conflicting
    BadConfiguration { reason: &'static str },
    /// A programming error occurred. This should be impossible.
    Programming { reason: &'static str },
    /// Attempted to re-configure the clocks, calling `init` twice.
    AlreadyConfigured,
}

impl ClockError {
    fn bad_config(reason: &'static str) -> Self {
        Self::BadConfiguration { reason }
    }

    fn prog_err(reason: &'static str) -> Self {
        Self::Programming { reason }
    }
}

struct ClockOperator<'a> {
    config: &'a ClockConfig,
    clocks: &'a mut Clocks,
    clkctl0: pac::Clkctl0,
    clkctl1: pac::Clkctl1,
    sysctl0: pac::Sysctl0,
    // sysctl1: pac::Sysctl1,
}

impl ClockOperator<'_> {
    /// Section 4.2: "Basic configuration"
    ///
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
    fn setup_clock_in(&mut self, sel: ClkInSelect) {
        // Optionally enable the clk_in
        match sel {
            ClkInSelect::Xtal {
                freq,
                bypass,
                low_power,
            } => {
                self.clkctl0
                    .sysoscctl0()
                    .write(|w| w.bypass_enable().bit(bypass).lp_enable().bit(low_power));
                self.clocks.clk_in = Some(freq);
            }
            ClkInSelect::ClkIn0_25 { freq, pin } => {
                pin.set_function(crate::gpio::Function::F7);
                self.clocks.clk_in = Some(freq);
            }
            ClkInSelect::ClkIn2_15 { freq, pin } => {
                pin.set_function(crate::gpio::Function::F7);
                self.clocks.clk_in = Some(freq);
            }
            ClkInSelect::ClkIn2_30 { freq, pin } => {
                pin.set_function(crate::gpio::Function::F5);
                self.clocks.clk_in = Some(freq);
            }
        }
    }

    // TODO: Make names consistent with `Clocks::ensure_*`

    /// ```text
    ///  ┌──────────┐
    ///  │16 MHz    │ 16m_irc
    ///  │oscillator│────────▶
    ///  └──────────┘
    ///        ▲
    ///        │
    /// PDRUNCFG0[14],
    /// PDSLEEPCFG0[14]
    /// ```
    fn ensure_16mhz_irc_active(&mut self) -> Result<u32, ClockError> {
        if !self.clocks._16m_irc.enabled {
            if !self.config.enable_16m_irc {
                return Err(ClockError::bad_config("16m_irc not enabled but required"));
            }
            self.sysctl0.pdruncfg0().modify(|_, w| w.sfro_pd().clear_bit());
            self.clocks._16m_irc.enabled = true;
        }
        Ok(self.clocks._16m_irc.frequency())
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
    fn ensure_48_60mhz_irc_active(&mut self) -> Result<u32, ClockError> {
        if let Some(freq) = self.clocks._48_60m_irc {
            Ok(freq)
        } else {
            // Select the 48/60m_irc clock speed
            self.clkctl0.ffroctl1().write(|w| w.update().update_safe_mode());
            let (variant, freq) = match self.config._48_60m_irc_select {
                _48_60mIrcSelect::Off => {
                    return Err(ClockError::bad_config("48/60m_irc required but disabled"));
                }
                _48_60mIrcSelect::Mhz48 => (pac::clkctl0::ffroctl0::TrimRange::Ffro48mhz, 48_000_000),
                _48_60mIrcSelect::Mhz60 => (pac::clkctl0::ffroctl0::TrimRange::Ffro60mhz, 60_000_000),
            };
            self.clkctl0.ffroctl0().write(|w| w.trim_range().variant(variant));
            self.clkctl0.ffroctl1().write(|w| w.update().normal_mode());

            // Enable
            self.sysctl0.pdruncfg0().modify(|_, w| w.ffro_pd().clear_bit());
            // NOTE: we know this is always a Some variant
            self.clocks._48_60m_irc = self.config._48_60m_irc_select.freq();
            Ok(freq)
        }
    }

    /// ```text
    ///  ┌──────────┐
    ///  │1 MHz low │
    ///  │power osc.│───▶ 1m_lposc
    ///  └──────────┘
    ///        ▲
    ///        │
    /// PDRUNCFG0[14],
    /// PDSLEEPCFG0[14]
    /// ```
    fn ensure_1mhz_lposc_active(&mut self) -> Result<u32, ClockError> {
        if !self.clocks._1m_lposc.enabled {
            if !self.config.enable_1m_lposc {
                return Err(ClockError::bad_config("1m_lposc disabled but required"));
            }
            self.sysctl0.pdruncfg0().modify(|_, w| w.lposc_pd().clear_bit());
            self.clocks._1m_lposc.enabled = true;
        }
        Ok(self.clocks._1m_lposc.frequency())
    }

    /// ```text
    ///             ┌───────────┐
    ///  rtcxin ───▶│RTC crystal│ 32k_clk
    /// rtcxout ───▶│oscillator │─────────▶
    ///             └───────────┘
    ///                   ▲
    ///                   │
    ///                Enable
    ///            OSC32KHZCTL0[0]
    /// ```
    fn ensure_32kclk_active(&mut self) -> Result<u32, ClockError> {
        if !self.clocks._32k_clk.enabled {
            if !self.config.enable_32k_clk {
                return Err(ClockError::bad_config("32k_clk required but not enabled"));
            }
            self.clkctl0.osc32khzctl0().write(|w| w.ena32khz().set_bit());
            self.clocks._32k_clk.enabled = true;
        }
        Ok(self.clocks._32k_clk.frequency())
    }

    /// Section 4.2.1: "Set up the Main PLL"
    ///
    /// > The Main PLL creates a stable output clock at a higher frequency than the input clock. If a
    /// > main clock is needed with a frequency higher than the default 12 MHz clock and the 16
    /// > MHz or 48/60 MHz clocks are not appropriate, use the PLL to boost the input frequency.
    /// > The PLL can be set up by calling an API supplied by NXP Semiconductors. Also see
    /// > Section 4.6.1 “PLLs”and Chapter 6 “RT6xx Power APIs”.
    ///
    /// Returns `Ok(None)` if the main PLL is disabled.
    /// Returns the frequency and MainPll selection if the main PLL is enabled
    fn setup_main_pll(&mut self, sel: Option<MainPll>) -> Result<Option<(u32, MainPll)>, ClockError> {
        // Turn off the PLL if it was running
        //
        // TODO(AJM): Do we need to reset to some default FIRST before we disable the PLL
        // to ensure we don't hang the system when we disable the PLL here?
        self.sysctl0.pdruncfg0_set().write(|w| {
            w.syspllldo_pd().set_pdruncfg0();
            w.syspllana_pd().set_pdruncfg0();
            w
        });

        // TODO(AJM): check for synchronization/wait after disabling PLL?

        //                     ┌─────┐
        //         16m_irc ───▶│000  │
        //          clk_in ───▶│001  │
        // 48/60m_irc_div2 ───▶│010  │──────▶
        //          "none" ───▶│111  │
        //                     └─────┘
        //                        ▲
        //                        │
        //            Sys PLL clock select
        //             SYSPLL0CLKSEL[2:0]
        let Some(sel) = sel else {
            self.clkctl0.syspll0clksel().write(|w| w.sel().none());
            return Ok(None);
        };

        if !(16..=22).contains(&sel.multiplier) {
            return Err(ClockError::bad_config("main pll multiplier out of allowed range"));
        }

        let pll_input_freq = match sel.clock_select {
            MainPllClockSelect::_16mIrc => self.ensure_16mhz_irc_active()?,
            MainPllClockSelect::ClkIn => self
                .clocks
                .clk_in
                .ok_or_else(|| ClockError::prog_err("We should have set clk_in by now"))?,
            MainPllClockSelect::_48_60MIrcDiv2 => self.ensure_48_60mhz_irc_active()?,
        };

        // Select the clock input we want
        self.clkctl0
            .syspll0clksel()
            .write(|w| unsafe { w.sel().bits(sel.clock_select as u8) });

        // Set the fractional part of the multiplier to 0
        // This means we're only using the integer multiplier as specified in the config
        self.clkctl0.syspll0num().write(|w| unsafe { w.num().bits(0x0) });
        self.clkctl0.syspll0denom().write(|w| unsafe { w.denom().bits(0x1) });

        self.clkctl0.syspll0ctl0().write(|w| {
            // No bypass. We're using the PFD.
            w.bypass().programmed_clk();
            // Clear the reset because after this we're fully configured
            w.reset().normal();
            // Set the user provided multiplier
            unsafe {
                w.mult().bits(sel.multiplier);
            }
            // For the first period we need the HOLDRINGOFF_ENA on
            w.holdringoff_ena().enable();
            w
        });

        // Turn on the PLL
        self.sysctl0.pdruncfg0_clr().write(|w| {
            w.syspllldo_pd().clr_pdruncfg0();
            w.syspllana_pd().clr_pdruncfg0();
            w
        });

        // Get the amount of us we need to wait
        let lock_time_div_2 = self.clkctl0.syspll0locktimediv2().read().locktimediv2().bits();
        cortex_m::asm::delay(WORST_CASE_TICKS_PER_US * lock_time_div_2 as u32);

        // For the second period we need the HOLDRINGOFF_ENA off
        self.clkctl0.syspll0ctl0().modify(|_, w| w.holdringoff_ena().dsiable());
        cortex_m::asm::delay(WORST_CASE_TICKS_PER_US * lock_time_div_2 as u32);

        // Output freq is just input times multiplier because the fractional part is hardcoded at 0
        Ok(Some((pll_input_freq * sel.multiplier as u32, sel)))
    }

    /// Section 4.2.2: "Configure the main clock and system clock" (part 1)
    ///
    /// ```text
    ///                      ┌────┐
    ///  48/60m_irc_div2 ───▶│00  │
    ///           clk_in ───▶│01  │                      ┌────┐
    ///         1m_lposc ───▶│10  │─────────────────────▶│00  │
    ///       48/60m_irc ───▶│11  │         16m_irc ┌───▶│01  │
    ///                      └────┘    ─────────────┘┌──▶│10  │─────▶ main_clk
    ///                         ▲      main_pll_clk  │┌─▶│11  │
    ///                         │      ──────────────┘│  └────┘
    ///           Main clock select A       32k_clk   │     ▲
    ///            MAINCLKSELA[1:0]    ───────────────┘     │
    ///                                       Main clock select B
    ///                                        MAINCLKSELB[1:0]
    /// ```
    fn setup_main_clock(&mut self) -> Result<(), ClockError> {
        self.clocks.main_clk = match self.config.main_clock_select {
            MainClockSelect::_48_60MIrcDiv4 => self.ensure_48_60mhz_irc_active()? / 4,
            MainClockSelect::ClkIn => {
                return Err(ClockError::bad_config(
                    "Main clock uses clk_in, but clk_in is not active",
                ));
            }
            MainClockSelect::_1mLposc => self.ensure_1mhz_lposc_active()?,
            MainClockSelect::_48_60MIrc => self.ensure_48_60mhz_irc_active()?,
            MainClockSelect::_16mIrc => self.ensure_16mhz_irc_active()?,
            MainClockSelect::MainPllClk => {
                // TODO: AJM, did I implement this?
                return Err(ClockError::bad_config(
                    "Main clock uses main_pll_clk, but main_pll_clk is not active",
                ));
            }
            MainClockSelect::_32kClk => self.ensure_32kclk_active()?,
        };

        // Select the main clock
        self.clkctl0
            .mainclksela()
            .write(|w| unsafe { w.bits((self.config.main_clock_select as u32 & 0b11_00) >> 2) });
        self.clkctl0
            .mainclkselb()
            .write(|w| unsafe { w.bits(self.config.main_clock_select as u32 & 0b00_11) });
        Ok(())
    }

    /// Section 4.2.2: "Configure the main clock and system clock" (part 2)
    ///                  ┌─────────┐
    ///                  │CPU Clock│  to CPU, AHB, APB, etc.
    ///             ┌───▶│Divider  │──────────────────────────────────────▶
    ///             │    └─────────┘           hclk
    ///             │         ▲
    ///             │         │
    ///             │  SYSCPUAHBCLKDIV
    ///             │  ┌─────────────┐
    ///             │  │ARM Trace    │ to ARM Trace function clock
    /// main_clk ───┼─▶│Clock Divider│────────────────────────────────────▶
    ///             │  └─────────────┘
    ///             │         ▲
    ///             │         │
    ///             │      PFC0DIV
    ///             │  ┌─────────────┐
    ///             │  │Systick Clock│                ┌─────┐
    ///             └─▶│Divider      │───────────────▶│000  │
    ///                └─────────────┘   1m_lposc┌───▶│001  │  to Systick
    ///                       ▲          ────────┘┌──▶│010  │─────────────▶
    ///                       │           32k_clk │┌─▶│011  │function clock
    ///                SYSTICKFCLKDIV    ─────────┘│┌▶│111  │
    ///                                   16m_irc  ││ └─────┘
    ///                                  ──────────┘│    ▲
    ///                                    "none"   │    │
    ///                                  ───────────┘    │
    ///                                                  │
    ///                                  SYSTICKFCLKSEL[2:0]
    fn setup_system_clock(&mut self) -> Result<(), ClockError> {
        let current_div = self.clkctl0.syscpuahbclkdiv().read().div();
        if current_div != self.config.sys_cpu_ahb_div {
            // "The clock being divided must be running for [reqflag] to change"
            if self.clocks.main_clk == 0 {
                return Err(ClockError::prog_err("main_clk not configured"));
            }
            self.clkctl0
                .syscpuahbclkdiv()
                .modify(|_r, w| unsafe { w.div().bits(self.config.sys_cpu_ahb_div) });
            // "Set when a change is made, clear when the change is complete"
            while self.clkctl0.syscpuahbclkdiv().read().reqflag().bit_is_set() {}
        }
        self.clocks.sys_cpu_ahb_clk = Some(self.clocks.main_clk / (self.config.sys_cpu_ahb_div as u32 + 1));

        // NOTE: We currently don't do any of the following:
        //
        // * Enable ARM Trace clock divider
        // * Enable Systick Clock divider, enable systick function clock
        // * Enable any of the CLKCTL0_PSCCTLn registers, these will be done
        //   later when peripherals are enabled
        Ok(())
    }

    /// ```text
    ///                 ┌─────────────┐
    ///                 │Main PLL     │  main_pll_clk
    /// main_pll ──────▶│Clock Divider│ ────────────▶
    ///  (pfd0)         └─────────────┘
    ///                        ▲
    ///                        │
    ///                  MAINPLLCLKDIV
    /// ```
    fn setup_pll_pfd0(&mut self, main_pll: &MainPll, pll_output_freq: u32) -> Result<(), ClockError> {
        let Some(div) = main_pll.pfd0_div else {
            self.disable_pll_pfd0();
            return Ok(());
        };
        if !(12..=35).contains(&div) {
            return Err(ClockError::bad_config("`pfd0_div` is out of the allowed range"));
        }

        self.clkctl0.syspll0pfd().modify(|_, w| {
            unsafe {
                w.pfd0().bits(div);
            }
            w.pfd0_clkrdy().set_bit();
            w.pfd0_clkgate().not_gated();
            w
        });
        while self.clkctl0.syspll0pfd().read().pfd0_clkrdy().bit_is_clear() {}

        let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

        if !(1..=256).contains(&main_pll.main_pll_clock_divider) {
            return Err(ClockError::bad_config(
                "`main_pll_clock_divider` is out of the allowed range",
            ));
        }

        // Halt and reset the div
        self.clkctl0.mainpllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.mainpllclkdiv().write(|w| {
            unsafe {
                w.div().bits((main_pll.main_pll_clock_divider - 1) as u8);
            }
            w.reset().set_bit()
        });
        while self.clkctl0.mainpllclkdiv().read().reqflag().bit_is_set() {}

        self.clocks.main_pll_clk = Some(pfd_freq / (main_pll.main_pll_clock_divider - 1) as u32);
        Ok(())
    }

    /// ```text
    ///                 ┌─────────────┐
    ///                 │DSP PLL      │   dsp_pll_clk
    /// main_pll ──────▶│Clock Divider│ ────────────▶
    ///  (pfd1)         └─────────────┘
    ///                        ▲
    ///                        │
    ///                  DSPPLLCLKDIV
    /// ```
    fn setup_pll_pfd1(&mut self, main_pll: &MainPll, pll_output_freq: u32) -> Result<(), ClockError> {
        let Some(div) = main_pll.pfd1_div else {
            self.disable_pll_pfd1();
            return Ok(());
        };
        if !(12..=35).contains(&div) {
            return Err(ClockError::bad_config("`pfd1_div` is out of the allowed range"));
        }

        self.clkctl0.syspll0pfd().modify(|_, w| {
            unsafe {
                w.pfd1().bits(div);
            }
            w.pfd1_clkrdy().set_bit();
            w.pfd1_clkgate().not_gated();
            w
        });
        while self.clkctl0.syspll0pfd().read().pfd1_clkrdy().bit_is_clear() {}

        let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

        if !(1..=256).contains(&main_pll.dsp_pll_clock_divider) {
            return Err(ClockError::bad_config(
                "`dsp_pll_clock_divider` is out of the allowed range",
            ));
        }

        // Halt and reset the div
        self.clkctl0.dsppllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.dsppllclkdiv().write(|w| {
            unsafe {
                w.div().bits((main_pll.dsp_pll_clock_divider - 1) as u8);
            }
            w.reset().set_bit()
        });
        while self.clkctl0.dsppllclkdiv().read().reqflag().bit_is_set() {}

        self.clocks.dsp_pll_clk = Some(pfd_freq / (main_pll.dsp_pll_clock_divider - 1) as u32);
        Ok(())
    }

    /// ```text
    ///                 ┌─────────────┐
    ///                 │AUX0 PLL     │  aux0_pll_clk
    /// main_pll ──────▶│Clock Divider│ ────────────▶
    ///  (pfd2)         └─────────────┘
    ///                        ▲
    ///                        │
    ///                  AUX0PLLCLKDIV
    /// ```
    fn setup_pll_pfd2(&mut self, main_pll: &MainPll, pll_output_freq: u32) -> Result<(), ClockError> {
        let Some(div) = main_pll.pfd2_div else {
            self.disable_pll_pfd2();
            return Ok(());
        };
        if !(12..=35).contains(&div) {
            return Err(ClockError::bad_config("`pfd2_div` is out of the allowed range"));
        }
        self.clkctl0.syspll0pfd().modify(|_, w| {
            unsafe {
                w.pfd2().bits(div);
            }
            w.pfd2_clkrdy().set_bit();
            w.pfd2_clkgate().not_gated();
            w
        });
        while self.clkctl0.syspll0pfd().read().pfd2_clkrdy().bit_is_clear() {}

        let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

        if !(1..=256).contains(&main_pll.aux0_pll_clock_divider) {
            return Err(ClockError::bad_config(
                "`aux0_pll_clock_divider` is out of the allowed range",
            ));
        }

        // Halt and reset the div
        self.clkctl0.aux0pllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.aux0pllclkdiv().write(|w| {
            unsafe {
                w.div().bits((main_pll.aux0_pll_clock_divider - 1) as u8);
            }
            w.reset().set_bit()
        });
        while self.clkctl0.aux0pllclkdiv().read().reqflag().bit_is_set() {}

        self.clocks.aux0_pll_clk = Some(pfd_freq / (main_pll.aux0_pll_clock_divider - 1) as u32);
        Ok(())
    }

    /// ```text
    ///                 ┌─────────────┐
    ///                 │AUX1 PLL     │  aux1_pll_clk
    /// main_pll ──────▶│Clock Divider│ ────────────▶
    ///  (pfd3)         └─────────────┘
    ///                        ▲
    ///                        │
    ///                  AUX1PLLCLKDIV
    /// ```
    fn setup_pll_pfd3(&mut self, main_pll: &MainPll, pll_output_freq: u32) -> Result<(), ClockError> {
        let Some(div) = main_pll.pfd3_div else {
            self.disable_pll_pfd3();
            return Ok(());
        };
        if !(12..=35).contains(&div) {
            return Err(ClockError::bad_config("`pfd3_div` is out of the allowed range"));
        }

        self.clkctl0.syspll0pfd().modify(|_, w| {
            unsafe {
                w.pfd3().bits(div);
            }
            w.pfd3_clkrdy().set_bit();
            w.pfd3_clkgate().not_gated();
            w
        });
        while self.clkctl0.syspll0pfd().read().pfd3_clkrdy().bit_is_clear() {}

        let pfd_freq = (pll_output_freq as u64 * 18 / div as u64) as u32;

        if !(1..=256).contains(&main_pll.aux1_pll_clock_divider) {
            return Err(ClockError::bad_config(
                "`aux1_pll_clock_divider` is out of the allowed range",
            ));
        }

        // Halt and reset the div
        self.clkctl0.aux1pllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.aux1pllclkdiv().write(|w| {
            unsafe {
                w.div().bits((main_pll.aux1_pll_clock_divider - 1) as u8);
            }
            w.reset().set_bit()
        });
        while self.clkctl0.aux1pllclkdiv().read().reqflag().bit_is_set() {}

        self.clocks.aux1_pll_clk = Some(pfd_freq / (main_pll.aux1_pll_clock_divider - 1) as u32);
        Ok(())
    }

    fn disable_pll_pfd0(&mut self) {
        self.clkctl0.mainpllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.syspll0pfd().modify(|_, w| w.pfd0_clkgate().gated());
        self.clocks.main_pll_clk = None;
    }

    fn disable_pll_pfd1(&mut self) {
        self.clkctl0.dsppllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.syspll0pfd().modify(|_, w| w.pfd1_clkgate().gated());
        self.clocks.dsp_pll_clk = None;
    }

    fn disable_pll_pfd2(&mut self) {
        self.clkctl0.aux0pllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.syspll0pfd().modify(|_, w| w.pfd2_clkgate().gated());
        self.clocks.aux0_pll_clk = None;
    }

    fn disable_pll_pfd3(&mut self) {
        self.clkctl0.aux1pllclkdiv().write(|w| w.halt().set_bit());
        self.clkctl0.syspll0pfd().modify(|_, w| w.pfd3_clkgate().gated());
        self.clocks.aux1_pll_clk = None;
    }

    fn ensure_main_pll_clk_active(&self) -> Result<u32, ClockError> {
        if let Some(f) = self.clocks.main_pll_clk {
            Ok(f)
        } else {
            Err(ClockError::BadConfiguration {
                reason: "main_pll_clk required but disabled",
            })
        }
    }

    /// ```text
    ///               ┌────────────────┐
    /// main_pll_clk  │PLL to Flexcomm │ frg_pll
    /// ─────────────▶│FRG Divider     │────────▶
    ///               └────────────────┘
    ///                        ▲
    ///                        │
    ///                  FRGPLLCLKDIV
    /// ```
    fn setup_frg_pll_div(&mut self) -> Result<(), ClockError> {
        let Some(div) = self.config.frg_clk_pll_div else {
            return Ok(());
        };
        let pll_freq = self.ensure_main_pll_clk_active()?;
        // TODO: Enforce max rate of 280_000_000
        if self.clkctl1.frgpllclkdiv().read().div() != div {
            self.clkctl1.frgpllclkdiv().modify(|_r, w| unsafe { w.div().bits(div) });
            while self.clkctl1.frgpllclkdiv().read().reqflag().bit_is_set() {}
        }
        self.clocks.frg_pll_clk = Some(pll_freq / (div as u32 + 1));
        Ok(())
    }

    /// ```text
    ///                     32k_clk
    ///                     ───────┐
    ///             ┌─────────┐    │  ┌─────┐
    ///             │divide by│    └─▶│000  │ 32k_wake_clk
    /// 1m_lposc ──▶│   32    │ ─────▶│001  │─────────────▶
    ///             └─────────┘    ┌─▶│111  │
    ///                     "none" │  └─────┘
    ///                     ───────┘     ▲
    ///                                  │
    ///                        WAKECLK32KHZSEL[2:0]
    /// ```
    fn setup_32k_wake_clk(&mut self) -> Result<u32, ClockError> {
        let freq = match self.config._32k_wake_clk_select {
            _32kWakeClkSelect::Off => 0,
            _32kWakeClkSelect::_32kClk => self.ensure_32kclk_active()?,
            _32kWakeClkSelect::Lp32k => {
                // Interestingly, 1_000_000 / 32 is 31250, NOT 32768. It's
                // only 4.8% off, though the 1mhz lposc is ALSO +/-10%.
                self.ensure_1mhz_lposc_active()? / 32
            }
        };
        self.clkctl0
            .wakeclk32khzsel()
            .write(|w| unsafe { w.sel().bits(self.config._32k_wake_clk_select as u8) });

        // TODO: Consistency "Some(0)" vs "None"
        if freq != 0 {
            self.clocks._32k_wake_clk = Some(freq);
        } else {
            self.clocks._32k_wake_clk = None;
        }
        Ok(freq)
    }

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
    pub fn setup_clock_out(&mut self) -> Result<u32, ClockError> {
        let Some(div) = self.config.clk_out_div else {
            self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().none());
            self.clocks.clk_out = None;
            return Ok(0);
        };

        let mut freq = match self.config.clk_out_select {
            ClockOutSource::M16Irc => {
                let freq = self.ensure_16mhz_irc_active()?;
                self.clkctl1.clkoutsel0().modify(|_r, w| w.sel().sfro_clk());
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().clkoutsel0_output());
                freq
            }
            ClockOutSource::ClkIn => {
                let freq = self.ensure_clk_in_active()?;
                self.clkctl1.clkoutsel0().modify(|_r, w| w.sel().xtalin_clk());
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().clkoutsel0_output());
                freq
            }
            ClockOutSource::M1Lposc => {
                let freq = self.ensure_1mhz_lposc_active()?;
                self.clkctl1.clkoutsel0().modify(|_r, w| w.sel().lposc());
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().clkoutsel0_output());
                freq
            }
            ClockOutSource::M4860Irc => {
                let freq = self.ensure_48_60mhz_irc_active()?;
                self.clkctl1.clkoutsel0().modify(|_r, w| w.sel().ffro_clk());
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().clkoutsel0_output());
                freq
            }
            ClockOutSource::MainClk => {
                let freq = self.clocks.main_clk;
                self.clkctl1.clkoutsel0().modify(|_r, w| w.sel().main_clk());
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().clkoutsel0_output());
                freq
            }
            ClockOutSource::DspMainClk => {
                return Err(ClockError::prog_err("dsp_main_clk not implemented"));
            }
            ClockOutSource::MainPllClk => {
                let freq = self.ensure_main_pll_clk_active()?;
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().main_pll_clk());
                freq
            }
            ClockOutSource::Aux0PllClk => {
                let freq = self.clocks.ensure_aux0_pll()?;
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().syspll0_aux0_pll_clk());
                freq
            }
            ClockOutSource::DspPllClk => {
                let freq = self.clocks.ensure_dsp_pll()?;
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().dsp_pll_clk());
                freq
            }
            ClockOutSource::Aux1PllClk => {
                let freq = self.clocks.ensure_aux0_pll()?;
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().syspll0_aux0_pll_clk());
                freq
            }
            ClockOutSource::AudioPllClk => {
                return Err(ClockError::prog_err("audio_pll_clk not implemented"));
            }
            ClockOutSource::K32Clk => {
                let freq = self.ensure_32kclk_active()?;
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().rtc_clk_32khz());
                freq
            }
            ClockOutSource::None => {
                self.clkctl1.clkoutsel1().modify(|_r, w| w.sel().none());
                return Ok(0);
            }
        };

        self.clkctl1
            .clkoutdiv()
            .modify(|_, w| w.halt().set_bit().reset().set_bit());
        self.clkctl1.clkoutdiv().modify(|_, w| unsafe { w.div().bits(div) });
        self.clkctl1.clkoutdiv().modify(|_, w| w.halt().clear_bit());
        while self.clkctl1.clkoutdiv().read().reqflag().bit_is_set() {}
        freq /= 1u32 + div as u32;
        self.clocks.clk_out = Some(freq);

        Ok(freq)
    }

    fn ensure_clk_in_active(&self) -> Result<u32, ClockError> {
        self.clocks
            .clk_in
            .ok_or_else(|| ClockError::bad_config("clk_in required but not configured"))
    }
}

/// In MOST cases, peripherals should determine their clock frequency by calling
/// `enable_and_reset::<T>`, which will return the clock frequency of their peripheral.
///
/// For cases where you REALLY want to view the full clock state (or at least the portion
/// covered by `init()`), this function can be used to view the clocks.
///
/// This function runs the provided closure *inside of a critical section*, so move with
/// purpose! You can clone-out the Clocks struct if necessary, or perform a more directed
/// query and return that instead.
///
/// Returns `None` if the clocks have not yet been initialized.
pub fn with_clocks<F: FnOnce(&Clocks) -> R, R>(f: F) -> Option<R> {
    critical_section::with(|cs| {
        let c = CLOCKS.borrow_ref(cs);
        let c = c.as_ref()?;
        Some(f(c))
    })
}

///Trait to expose perph clocks
pub(crate) trait SealedSysconPeripheral {
    type SysconPeriphConfig: SPConfHelper;

    fn enable_perph_clock();
    fn reset_perph();
    fn disable_perph_clock();
}

/// Clock and Reset control for peripherals
#[allow(private_bounds)]
pub trait SysconPeripheral: SealedSysconPeripheral + 'static {}

/// Enables peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
pub(crate) fn enable<T: SysconPeripheral>(cfg: &T::SysconPeriphConfig) -> Result<u32, ClockError> {
    T::enable_perph_clock();
    let freq = critical_section::with(|cs| {
        let clocks = CLOCKS.borrow_ref(cs);
        let clocks = clocks.as_ref().ok_or(ClockError::prog_err("didn't call init"))?;
        cfg.post_enable_config(clocks)
    })?;
    Ok(freq)
}

/// Enables and resets peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
pub(crate) fn enable_and_reset<T: SysconPeripheral>(cfg: &T::SysconPeriphConfig) -> Result<u32, ClockError> {
    let freq = enable::<T>(cfg)?;
    T::reset_perph();
    Ok(freq)
}

/// Disables peripheral `T`.
///
/// # Safety
///
/// Peripheral must not be in use.
pub fn disable<T: SysconPeripheral>() {
    T::disable_perph_clock();
}

macro_rules! impl_perph_clk {
    (
        $peripheral:ident,
        $clkctl:ident,
        $clkreg:ident,
        $rstctl:ident,
        $rstreg:ident,
        $bit:expr,
        $cfgty:ty
    ) => {
        impl SealedSysconPeripheral for crate::peripherals::$peripheral {
            type SysconPeriphConfig = $cfgty;

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

use periph_helpers::sealed::UnimplementedConfig;

// These should enabled once the relevant peripherals are implemented.
// impl_perph_clk!(GPIOINTCTL, Clkctl1, pscctl2, Rstctl1, prstctl2, 30, UnimplementedConfig, unimpld_cfg);
// impl_perph_clk!(OTP, Clkctl0, pscctl0, Rstctl0, prstctl0, 17, UnimplementedConfig, unimpld_cfg);

// impl_perph_clk!(ROM_CTL_128KB, Clkctl0, pscctl0, Rstctl0, prstctl0, 2, UnimplementedConfig, unimpld_cfg);
// impl_perph_clk!(USBHS_SRAM, Clkctl0, pscctl0, Rstctl0, prstctl0, 23, UnimplementedConfig, unimpld_cfg);

impl_perph_clk!(PIMCTL, Clkctl1, pscctl2, Rstctl1, prstctl2, 31, NoConfig);
// TODO: ACMP DOES have upstream clock configuration required
impl_perph_clk!(ACMP, Clkctl0, pscctl1, Rstctl0, prstctl1, 15, UnimplementedConfig);
// TODO: ADC0 DOES have upstream clock configuration required
impl_perph_clk!(ADC0, Clkctl0, pscctl1, Rstctl0, prstctl1, 16, AdcConfig);
// TODO: Ensure that CASPER SRAM is also enabled prior to starting CASPER?
impl_perph_clk!(CASPER, Clkctl0, pscctl0, Rstctl0, prstctl0, 9, UnimplementedConfig);
impl_perph_clk!(CRC, Clkctl1, pscctl1, Rstctl1, prstctl1, 16, NoConfig);
impl_perph_clk!(
    CTIMER0_COUNT_CHANNEL0,
    Clkctl1,
    pscctl2,
    Rstctl1,
    prstctl2,
    0,
    CtimerConfig
);
impl_perph_clk!(
    CTIMER1_COUNT_CHANNEL0,
    Clkctl1,
    pscctl2,
    Rstctl1,
    prstctl2,
    1,
    CtimerConfig
);
impl_perph_clk!(
    CTIMER2_COUNT_CHANNEL0,
    Clkctl1,
    pscctl2,
    Rstctl1,
    prstctl2,
    2,
    CtimerConfig
);
impl_perph_clk!(
    CTIMER3_COUNT_CHANNEL0,
    Clkctl1,
    pscctl2,
    Rstctl1,
    prstctl2,
    3,
    CtimerConfig
);
impl_perph_clk!(
    CTIMER4_COUNT_CHANNEL0,
    Clkctl1,
    pscctl2,
    Rstctl1,
    prstctl2,
    4,
    CtimerConfig
);

impl_perph_clk!(DMA0, Clkctl1, pscctl1, Rstctl1, prstctl1, 23, NoConfig);
impl_perph_clk!(DMA1, Clkctl1, pscctl1, Rstctl1, prstctl1, 24, NoConfig);
// TODO: DMIC DOES have upstream clock configuration required
impl_perph_clk!(DMIC0, Clkctl1, pscctl0, Rstctl1, prstctl0, 24, UnimplementedConfig);
// TODO: ESPI DOES have upstream clock configuration required
#[cfg(feature = "_espi")]
impl_perph_clk!(ESPI, Clkctl0, pscctl1, Rstctl0, prstctl1, 7, UnimplementedConfig);

impl_perph_clk!(FLEXCOMM0, Clkctl1, pscctl0, Rstctl1, prstctl0, 8, FlexcommConfig);
impl_perph_clk!(FLEXCOMM1, Clkctl1, pscctl0, Rstctl1, prstctl0, 9, FlexcommConfig);
impl_perph_clk!(FLEXCOMM14, Clkctl1, pscctl0, Rstctl1, prstctl0, 22, FlexcommConfig14);
impl_perph_clk!(FLEXCOMM15, Clkctl1, pscctl0, Rstctl1, prstctl0, 23, FlexcommConfig15);
impl_perph_clk!(FLEXCOMM2, Clkctl1, pscctl0, Rstctl1, prstctl0, 10, FlexcommConfig);
impl_perph_clk!(FLEXCOMM3, Clkctl1, pscctl0, Rstctl1, prstctl0, 11, FlexcommConfig);
impl_perph_clk!(FLEXCOMM4, Clkctl1, pscctl0, Rstctl1, prstctl0, 12, FlexcommConfig);
impl_perph_clk!(FLEXCOMM5, Clkctl1, pscctl0, Rstctl1, prstctl0, 13, FlexcommConfig);
impl_perph_clk!(FLEXCOMM6, Clkctl1, pscctl0, Rstctl1, prstctl0, 14, FlexcommConfig);
impl_perph_clk!(FLEXCOMM7, Clkctl1, pscctl0, Rstctl1, prstctl0, 15, FlexcommConfig);

// NOTE: FlexSPI doesn't *really* have a normal clock setup, it has an OTP interface
// area, however it is also often configured directly by the FCB setup. We'll leave
// it to the [`flexspi`](crate::flexspi) module to handle the more active setup parts.
impl_perph_clk!(FLEXSPI, Clkctl0, pscctl0, Rstctl0, prstctl0, 16, NoConfig);
// TODO: FREQME has reference clock selection that needs to be configured
impl_perph_clk!(FREQME, Clkctl1, pscctl1, Rstctl1, prstctl1, 31, UnimplementedConfig);
impl_perph_clk!(HASHCRYPT, Clkctl0, pscctl0, Rstctl0, prstctl0, 10, NoConfig);
impl_perph_clk!(HSGPIO0, Clkctl1, pscctl1, Rstctl1, prstctl1, 0, NoConfig);
impl_perph_clk!(HSGPIO1, Clkctl1, pscctl1, Rstctl1, prstctl1, 1, NoConfig);
impl_perph_clk!(HSGPIO2, Clkctl1, pscctl1, Rstctl1, prstctl1, 2, NoConfig);
impl_perph_clk!(HSGPIO3, Clkctl1, pscctl1, Rstctl1, prstctl1, 3, NoConfig);
impl_perph_clk!(HSGPIO4, Clkctl1, pscctl1, Rstctl1, prstctl1, 4, NoConfig);
impl_perph_clk!(HSGPIO5, Clkctl1, pscctl1, Rstctl1, prstctl1, 5, NoConfig);
impl_perph_clk!(HSGPIO6, Clkctl1, pscctl1, Rstctl1, prstctl1, 6, NoConfig);
impl_perph_clk!(HSGPIO7, Clkctl1, pscctl1, Rstctl1, prstctl1, 7, NoConfig);
// TODO: I3C DOES have clock div/sel requirements
impl_perph_clk!(I3C0, Clkctl1, pscctl2, Rstctl1, prstctl2, 16, UnimplementedConfig);
impl_perph_clk!(MRT0, Clkctl1, pscctl2, Rstctl1, prstctl2, 8, NoConfig);
impl_perph_clk!(MU_A, Clkctl1, pscctl1, Rstctl1, prstctl1, 28, NoConfig);
// TODO: OS Event Timer has clock selection requirements
impl_perph_clk!(OS_EVENT, Clkctl1, pscctl0, Rstctl1, prstctl0, 27, OsEventConfig);
// TODO: As far as I can tell POWERQUAD doesn't require any additional clocking/setup?
// I'm not super confident about that, but we don't support it yet anyway
impl_perph_clk!(POWERQUAD, Clkctl0, pscctl0, Rstctl0, prstctl0, 8, NoConfig);
// TODO: PUF has it's own SRAM that needs to be enabled first
impl_perph_clk!(PUF, Clkctl0, pscctl0, Rstctl0, prstctl0, 11, UnimplementedConfig);
// NOTE: "RNG" *appears* to be the TRNG, as far as I can tell.
impl_perph_clk!(RNG, Clkctl0, pscctl0, Rstctl0, prstctl0, 12, NoConfig);
// TODO: We need to ensure the RTC oscillator is enabled (I think)
impl_perph_clk!(RTC, Clkctl1, pscctl2, Rstctl1, prstctl2, 7, UnimplementedConfig);
impl_perph_clk!(SCT0, Clkctl0, pscctl0, Rstctl0, prstctl0, 24, Sct0Config);
impl_perph_clk!(SECGPIO, Clkctl0, pscctl1, Rstctl0, prstctl1, 24, NoConfig);
impl_perph_clk!(SEMA42, Clkctl1, pscctl1, Rstctl1, prstctl1, 29, NoConfig);
// TODO: USBHSD (Device) DOES have clock setup requirements, and maybe SRAM requirements
impl_perph_clk!(USBHSD, Clkctl0, pscctl0, Rstctl0, prstctl0, 21, UnimplementedConfig);
// TODO: USBHSH (Host) DOES have clock setup requirements, and maybe SRAM requirements
impl_perph_clk!(USBHSH, Clkctl0, pscctl0, Rstctl0, prstctl0, 22, UnimplementedConfig);
// TODO: USBPHY has a lot of clock setup required
impl_perph_clk!(USBPHY, Clkctl0, pscctl0, Rstctl0, prstctl0, 20, UnimplementedConfig);
// TODO: USDHCn have clock and RAM setup requirements
impl_perph_clk!(USDHC0, Clkctl0, pscctl1, Rstctl0, prstctl1, 2, UnimplementedConfig);
impl_perph_clk!(USDHC1, Clkctl0, pscctl1, Rstctl0, prstctl1, 3, UnimplementedConfig);
// TODO: UTICK0 (Micro-Tick) has clock selection requirements
impl_perph_clk!(UTICK0, Clkctl0, pscctl2, Rstctl0, prstctl2, 0, UnimplementedConfig);
impl_perph_clk!(WDT0, Clkctl0, pscctl2, Rstctl0, prstctl2, 1, WdtConfig);
impl_perph_clk!(WDT1, Clkctl1, pscctl2, Rstctl1, prstctl2, 10, WdtConfig);

// Diagrams without homes (yet)
//
// -----
//
//                     ┌─────┐       ┌────────────┐
//         16m_irc ───▶│000  │       │            │      ┌─────────────┐
//          clk_in ───▶│001  │       │ Audio      │      │Audio PLL    │  audio_pll_clk
// 48/60m_irc_div2 ───▶│010  │──────▶│ PLL   PFD0 │─────▶│Clock Divider│ ────────────▶
//          "none" ───▶│111  │       │            │      └─────────────┘
//                     └─────┘       └────────────┘             ▲
//                        ▲                 ▲                   │
//                        │                 │             AUDIOPLLCLKDIV
//          Audio PLL clock select  Audio PLL settings
//           AUDIOPLL0CLKSEL[2:0]      AUDIOPLL0xx
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
