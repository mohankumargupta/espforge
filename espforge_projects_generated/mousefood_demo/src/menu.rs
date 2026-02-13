use core::marker::PhantomData;
use esp_hal::delay::Delay;
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListItem, ListState};
use ratatui::{Frame, Terminal};

extern crate alloc;
use alloc::vec::Vec;

pub struct MenuApp<B: Backend> {
    items: [&'static str; 3],
    state: ListState,
    _phantom: PhantomData<B>,
}

impl<B: Backend> MenuApp<B> {
    pub fn new() -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        
        Self {
            items: ["One", "Two", "Three"],
            state,
            _phantom: PhantomData,
        }
    }

    pub fn run(&mut self, terminal: &mut Terminal<B>, delay: &Delay) {
        loop {
            // if button.was_pressed() {
            //     self.next();
            //     // Exit after cycling through all items once
            //     if self.state.selected() == Some(0) {
            //         return;
            //     }
            // }

            terminal.draw(|frame| self.draw(frame)).unwrap();
            delay.delay_millis(33);
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn draw(&mut self, frame: &mut Frame) {
        let [top_area, footer_area] = 
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)])
                .areas(frame.area());

        // Create list items
        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|item| ListItem::new(*item))
            .collect();

        // Create the list widget
        let list = List::new(items)
            .block(Block::bordered().title("Menu"))
            .highlight_style(
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD)
            )
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, top_area, &mut self.state);

        let footer = Line::raw("[S1] to select next item").centered().gray();
        frame.render_widget(footer, footer_area);
    }
}

impl<B: Backend> Default for MenuApp<B> {
    fn default() -> Self {
        Self::new()
    }
}
