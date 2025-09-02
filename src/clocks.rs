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
    /// Clock divider for the `aux_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub aux_pll_clock_divider: u16,
    /// Clock divider for the `aux1_pll_clk`.
    ///
    /// Allowed range: `1..=256`.
    pub aux1_pll_clock_divider: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MainPllClockSelect {
    _16mIrc = 0b000,
    ClkIn = 0b001,
    _48_60MIrcDiv2 = 0b010,
}

#[derive(Clone, Copy, Debug)]
pub enum ClkInSelect {
    Xtal { freq: u32, bypass: bool, low_power: bool },
    ClkIn0_25 { freq: u32, pin: PIO0_25 },
    ClkIn2_15 { freq: u32, pin: PIO2_15 },
    ClkIn2_30 { freq: u32, pin: PIO2_30 },
}

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

        assert!(
            (16..=22).contains(&main_pll.multiplier),
            "main pll multiplier out of allowed range"
        );
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

        // TODO: More asserts, PFDs and setting the values in the clocks struct
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
