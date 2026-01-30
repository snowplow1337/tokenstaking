// lib.rs
use anchor_lang::prelude::*;
use anchor_lang::solana_program::program::invoke_signed;
use anchor_lang::solana_program::system_instruction;
use anchor_spl::{
    associated_token::AssociatedToken,
    metadata::{self, Metadata, mpl_token_metadata::types::DataV2},
    token::{self, Mint, Token, TokenAccount, Transfer},
};

declare_id!("Stake11111111111111111111111111111111111111");

#[program]
pub mod staking_program {
    use super::*;

    pub fn initialize_stake_pool(ctx: Context<InitializeStakePool>, stake_rate: u64) -> Result<()> {
        let pool = &mut ctx.accounts.stake_pool;
        pool.authority = ctx.accounts.authority.key();
        pool.token_mint = ctx.accounts.token_mint.key();
        pool.total_staked = 0;
        pool.stake_rate = stake_rate;
        pool.bump = ctx.bumps.stake_pool;
        Ok(())
    }

    pub fn stake_tokens(ctx: Context<StakeTokens>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.stake_pool;
        let user_stake = &mut ctx.accounts.user_stake;

        // Transfer tokens from user to the staking program
        let cpi_accounts = Transfer {
            from: ctx.accounts.user_token_account.to_account_info(),
            to: ctx.accounts.pool_token_account.to_account_info(),
            authority: ctx.accounts.user_authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount)?;

        // Update pool state
        pool.total_staked += amount;
        
        // Update user stake record
        user_stake.amount = amount;
        user_stake.staked_at = Clock::get()?.unix_timestamp;
        user_stake.reward_debt = 0;

        Ok(())
    }

    pub fn unstake_tokens(ctx: Context<UnstakeTokens>, amount: u64) -> Result<()> {
        let pool = &mut ctx.accounts.stake_pool;
        let user_stake = &mut ctx.accounts.user_stake;

        // Check if user has enough staked tokens
        require!(user_stake.amount >= amount, StakingError::InsufficientStake);

        // Calculate rewards (simplified)
        let reward = calculate_reward(pool, user_stake)?;
        
        // Transfer rewards to user
        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, amount + reward)?;

        // Update pool state
        pool.total_staked -= amount;
        
        // Update user stake record
        user_stake.amount -= amount;

        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let pool = &mut ctx.accounts.stake_pool;
        let user_stake = &mut ctx.accounts.user_stake;

        // Calculate rewards
        let reward = calculate_reward(pool, user_stake)?;
        
        // Transfer rewards to user
        let cpi_accounts = Transfer {
            from: ctx.accounts.pool_token_account.to_account_info(),
            to: ctx.accounts.user_token_account.to_account_info(),
            authority: ctx.accounts.pool_authority.to_account_info(),
        };
        let cpi_program = ctx.accounts.token_program.key();
        let cpi_ctx = CpiContext::new(cpi_program, cpi_accounts);
        token::transfer(cpi_ctx, reward)?;

        // Update reward debt
        user_stake.reward_debt += reward;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct InitializeStakePool<'info> {
    #[account(
        init,
        payer = authority,
        space = 8 + std::mem::size_of::<StakePool>(),
        seeds = [b"stake_pool", token_mint.key().as_ref()],
        bump
    )]
    pub stake_pool: Account<'info, StakePool>,
    pub token_mint: Account<'info, Mint>,
    #[account(mut)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct StakeTokens<'info> {
    #[account(
        mut,
        seeds = [b"stake_pool", token_mint.key().as_ref()],
        bump = stake_pool.bump
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        init_if_needed,
        payer = user_authority,
        space = 8 + std::mem::size_of::<UserStake>(),
        seeds = [b"user_stake", user_authority.key().as_ref(), stake_pool.key().as_ref()],
        bump
    )]
    pub user_stake: Account<'info, UserStake>,
    #[account(
        mut,
        constraint = pool_token_account.mint == stake_pool.token_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,
    #[account(
        constraint = user_token_account.mint == stake_pool.token_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    pub user_authority: Signer<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

#[derive(Accounts)]
pub struct UnstakeTokens<'info> {
    #[account(
        mut,
        seeds = [b"stake_pool", token_mint.key().as_ref()],
        bump = stake_pool.bump
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        mut,
        seeds = [b"user_stake", user_authority.key().as_ref(), stake_pool.key().as_ref()],
        bump = user_stake.bump
    )]
    pub user_stake: Account<'info, UserStake>,
    #[account(
        mut,
        constraint = pool_token_account.mint == stake_pool.token_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,
    #[account(
        constraint = user_token_account.mint == stake_pool.token_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    pub user_authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"stake_pool", stake_pool.token_mint.key().as_ref()],
        bump = stake_pool.bump
    )]
    pub pool_authority: Account<'info, StakePool>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(
        mut,
        seeds = [b"stake_pool", token_mint.key().as_ref()],
        bump = stake_pool.bump
    )]
    pub stake_pool: Account<'info, StakePool>,
    #[account(
        mut,
        seeds = [b"user_stake", user_authority.key().as_ref(), stake_pool.key().as_ref()],
        bump = user_stake.bump
    )]
    pub user_stake: Account<'info, UserStake>,
    #[account(
        mut,
        constraint = pool_token_account.mint == stake_pool.token_mint
    )]
    pub pool_token_account: Account<'info, TokenAccount>,
    #[account(
        constraint = user_token_account.mint == stake_pool.token_mint
    )]
    pub user_token_account: Account<'info, TokenAccount>,
    pub user_authority: Signer<'info>,
    #[account(
        mut,
        seeds = [b"stake_pool", stake_pool.token_mint.key().as_ref()],
        bump = stake_pool.bump
    )]
    pub pool_authority: Account<'info, StakePool>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct StakePool {
    pub authority: Pubkey,
    pub token_mint: Pubkey,
    pub total_staked: u64,
    pub stake_rate: u64, // Rewards per second
    pub bump: u8,
}

#[account]
pub struct UserStake {
    pub amount: u64,
    pub staked_at: i64,
    pub reward_debt: u64,
    pub bump: u8,
}

fn calculate_reward(pool: &mut StakePool, user_stake: &UserStake) -> Result<u64> {
    let current_time = Clock::get()?.unix_timestamp;
    let time_passed = (current_time - user_stake.staked_at) as u64;
    
    // Simple reward calculation: stake_amount * rate * time
    let reward = (user_stake.amount * pool.stake_rate * time_passed) / 1000; // Adjust denominator as needed
    
    Ok(reward)
}

#[error_code]
pub enum StakingError {
    #[msg("Insufficient stake amount")]
    InsufficientStake,
}
