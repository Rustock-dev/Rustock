//! rskj `PegUtilsLegacy.getTransactionType`: what a Bitcoin transaction
//! registered below the pegout index's height is to the Bridge, a peg-in, a
//! migration, a pegout, or nothing it registers.
//!
//! rskj tells an input that spends a federation in two ways. Its scriptSig
//! spends the federation's script, its signatures checked
//! (`scriptCorrectlySpendsTx`, run as bitcoinj runs it in
//! [`super::script_spend`]). Or the redeem script it carries is the
//! federation's: as it is before RSKIP201, and from RSKIP201 the standard
//! multisig it holds, which bitcoinj's parser reads out of an ERP or Flyover
//! redeem script ([`standard_redeem_script`]). Where bitcoinj throws something
//! the classification doesn't catch, the call fails.

use bitcoin::Transaction;

use super::peg::{
    federation_output_hash160, p2sh_output_script_program, redeem_script_hash160_pub,
};
use super::rskj_sender_compat::{
    redeem_script_of_input, standard_redeem_script, written_back, Written,
};
use super::script_spend::script_correctly_spends_tx;

/// A standard multisig federation's format version.
const STANDARD_MULTISIG_FEDERATION: u64 = 1000;
/// A P2SH-P2WSH ERP federation's format version.
const P2SH_P2WSH_ERP_FEDERATION: u64 = 4000;

/// The `ScriptException` a federation with no standard redeem script throws
/// where nothing catches it.
const NO_STANDARD_REDEEM: &str = "ScriptException reading a federation's standard redeem script";

/// What `getTransactionType` answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PegType {
    /// From RSKIP199, an input spends the old federation
    /// (`txIsFromOldFederation`): a pegout or migration.
    FromOldFederation,
    PegIn,
    Migration,
    Pegout,
    /// None of them: refused as a peg-in for one of its inputs, or for what it
    /// pays the live federations.
    Unknown {
        refused_for_an_input: bool,
    },
}

/// A live federation, as `getTransactionType` reads it.
pub(crate) struct LiveFederation<'a> {
    /// `getP2SHScript`: the script its UTXOs are paid to.
    pub p2sh: &'a [u8],
    /// `getRedeemScript`.
    pub redeem: &'a [u8],
    /// Its stored format version.
    pub format: u64,
}

impl LiveFederation<'_> {
    /// `PegUtilsLegacy.getFederationStandardRedeemScript`: a standard multisig
    /// federation's redeem script, an ERP federation's default multisig
    /// (`ErpFederation.getDefaultRedeemScript`), which bitcoinj's parser reads
    /// out of its redeem script. `Ok(None)` is that parser's `ScriptException`
    /// where it has none to give: for testnet's hard-coded ERP script, which a
    /// non-standard ERP federation there has before RSKIP284.
    fn standard_redeem(&self) -> Result<Option<Written>, &'static str> {
        if self.format == STANDARD_MULTISIG_FEDERATION {
            return Ok(Some(Ok(self.redeem.to_vec())));
        }
        standard_redeem_script(self.redeem)
    }

    /// `PegUtilsLegacy.getFederationStandardP2SHScript`: the P2SH script of
    /// that, a P2SH-P2WSH federation's P2SH-P2WSH one. What it can't read
    /// throws here where nothing catches it.
    fn standard_p2sh(&self) -> Result<Vec<u8>, &'static str> {
        let standard = self.standard_redeem()?.ok_or(NO_STANDARD_REDEEM)??;
        Ok(p2sh_output_script_program(&federation_output_hash160(
            &standard,
            self.format,
        )))
    }
}

/// What `getTransactionType` reads besides the transaction.
pub(crate) struct LegacyRules<'a> {
    pub rskip199: bool,
    pub rskip201: bool,
    pub rskip305: bool,
    /// The old federation's P2SH script (`getOldFederationAddress`).
    pub old_federation_p2sh: &'a [u8],
    pub active: LiveFederation<'a>,
    pub retiring: Option<LiveFederation<'a>>,
    /// `getLastRetiredFederationP2SHScript`, from RSKIP186.
    pub retired_p2sh: Option<&'a [u8]>,
    /// `isValidPegInTx`'s last test, of what the transaction pays the live
    /// federations: below the minimum peg-in value, from RSKIP293 in any one
    /// output, before it in total.
    pub live_below_minimum: bool,
    /// The same of what it pays the active federation alone.
    pub active_below_minimum: bool,
}

impl LegacyRules<'_> {
    /// `PegUtilsLegacy.getTransactionType`. `Err` is a throw nothing catches.
    pub(crate) fn transaction_type(&self, tx: &Transaction) -> Result<PegType, &'static str> {
        if self.rskip199
            && (0..tx.input.len())
                .any(|i| script_correctly_spends_tx(tx, i, self.old_federation_p2sh))
        {
            return Ok(PegType::FromOldFederation);
        }
        let live: Vec<&LiveFederation> = std::iter::once(&self.active)
            .chain(self.retiring.as_ref())
            .collect();
        let refused_for_an_input = self.refused_for_an_input(tx, &live, self.retired_p2sh)?;
        if !refused_for_an_input && !self.live_below_minimum {
            return Ok(PegType::PegIn);
        }
        if self.is_migration(tx)? {
            return Ok(PegType::Migration);
        }
        let live_standard_p2sh = live
            .iter()
            .map(|federation| federation.standard_p2sh())
            .collect::<Result<Vec<_>, _>>()?;
        if self.is_pegout(tx, &live_standard_p2sh)? {
            return Ok(PegType::Pegout);
        }
        Ok(PegType::Unknown {
            refused_for_an_input,
        })
    }

    /// The inputs `isValidPegInTx` refuses a peg-in for: one that spends a
    /// live federation's script or the last retired one's, its signatures
    /// checked, or from RSKIP201 one whose redeem script's standard multisig
    /// is a live federation's, or as P2SH is the last retired one's script.
    /// It reads each input's redeem script whatever the upgrade, and a
    /// scriptSig bitcoinj can't parse throws there.
    fn refused_for_an_input(
        &self,
        tx: &Transaction,
        live: &[&LiveFederation],
        retired_p2sh: Option<&[u8]>,
    ) -> Result<bool, &'static str> {
        for (index, input) in tx.input.iter().enumerate() {
            if live
                .iter()
                .any(|federation| script_correctly_spends_tx(tx, index, federation.p2sh))
                || retired_p2sh.is_some_and(|script| script_correctly_spends_tx(tx, index, script))
            {
                return Ok(true);
            }
            let redeem = redeem_script_of_input(input.script_sig.as_bytes(), input.witness.last())?;
            let (true, Some(redeem)) = (self.rskip201, redeem) else {
                continue;
            };
            if standard_redeem_is_a_federations(&redeem, live, retired_p2sh)? == Some(true) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `PegUtilsLegacy.isMigrationTx`: an input that is a pegout of the
    /// retiring or the last retired federation, by their standard scripts, in
    /// a transaction that is a peg-in to the active federation alone.
    fn is_migration(&self, tx: &Transaction) -> Result<bool, &'static str> {
        if self.retired_p2sh.is_none() && self.retiring.is_none() {
            return Ok(false);
        }
        let mut scripts = Vec::new();
        if let Some(retired) = self.retired_p2sh {
            scripts.push(retired.to_vec());
        }
        if let Some(retiring) = &self.retiring {
            scripts.push(retiring.standard_p2sh()?);
        }
        let from_retiring_or_retired = self.is_pegout(tx, &scripts)?;
        let to_active =
            !self.refused_for_an_input(tx, &[&self.active], None)? && !self.active_below_minimum;
        Ok(from_retiring_or_retired && to_active)
    }

    /// `PegUtilsLegacy.isPegOutTx`: an input whose redeem script's P2SH
    /// script, or from RSKIP305 its P2SH-P2WSH one, is among `scripts`. The
    /// redeem script is taken as it is before RSKIP201 and from RSKIP201 as
    /// its standard multisig; an input with neither is passed over.
    fn is_pegout(&self, tx: &Transaction, scripts: &[Vec<u8>]) -> Result<bool, &'static str> {
        for input in &tx.input {
            let Some(redeem) =
                redeem_script_of_input(input.script_sig.as_bytes(), input.witness.last())?
            else {
                continue;
            };
            let written = if self.rskip201 {
                standard_redeem_script(&redeem)?
            } else {
                written_back(&redeem)
            };
            let Some(written) = written else {
                continue;
            };
            let program = written?;
            let p2sh = p2sh_output_script_program(&redeem_script_hash160_pub(&program));
            let p2sh_p2wsh = p2sh_output_script_program(&federation_output_hash160(
                &program,
                P2SH_P2WSH_ERP_FEDERATION,
            ));
            if scripts.contains(&p2sh) || (self.rskip305 && scripts.contains(&p2sh_p2wsh)) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// The try block of `isValidPegInTx` from RSKIP201: whether `redeem`'s
/// standard multisig is a live federation's, or its P2SH script the last
/// retired federation's. `None` is the `ScriptException` it catches, from the
/// input's redeem script or from a federation's. `Script.equals` writes the
/// federation's program and then the input's, and the input's P2SH script
/// writes it again where no federation's was compared.
fn standard_redeem_is_a_federations(
    redeem: &[u8],
    live: &[&LiveFederation],
    retired_p2sh: Option<&[u8]>,
) -> Result<Option<bool>, &'static str> {
    let Some(written) = standard_redeem_script(redeem)? else {
        return Ok(None);
    };
    for federation in live {
        let Some(standard) = federation.standard_redeem()? else {
            return Ok(None);
        };
        if standard? == written.clone()? {
            return Ok(Some(true));
        }
    }
    let p2sh = p2sh_output_script_program(&redeem_script_hash160_pub(&written?));
    Ok(Some(retired_p2sh.is_some_and(|retired| p2sh == retired)))
}
